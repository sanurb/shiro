//! `DoclingParser` — structured PDF parser using Docling as a subprocess.
//!
//! Invokes the `docling` CLI to produce a `DoclingDocument` JSON, then
//! translates it into shiro's canonical IR via [`crate::translate`].
//!
//! **Subprocess boundary:** Docling is a Python tool. We shell out to it
//! rather than embedding Python, keeping the dependency boundary clean.
//! The adapter is optional — if `docling` is not installed, parse fails
//! with a clear error.
//!
//! **Determinism:** For the same Docling version + input bytes, output is
//! identical. The parser version tracks both adapter version and the Docling
//! CLI version discovered at construction time.

use std::process::Command;

use shiro_core::ir::Document;
use shiro_core::ports::Parser;
use shiro_core::ShiroError;

use crate::schema::DoclingDocument;
use crate::translate;

/// Parser version of the shiro-docling adapter.
///
/// Bump when any output-affecting change is made to the translation layer,
/// subprocess invocation, or post-processing logic (ADR-004).
const ADAPTER_VERSION: u32 = 1;

/// Structured PDF parser backed by the Docling CLI.
///
/// # Construction
///
/// Use [`DoclingParser::new`] for default settings, or
/// [`DoclingParser::with_binary`] to specify a custom `docling` binary path.
///
/// # Requirements
///
/// The `docling` CLI must be installed and available on `$PATH` (or at the
/// specified binary path). Typically installed via `pip install docling`.
#[derive(Debug, Clone)]
pub struct DoclingParser {
    /// Path to the `docling` binary.
    binary: String,

    /// Whether to disable OCR (faster, less complete for scanned docs).
    no_ocr: bool,
}

impl DoclingParser {
    /// Create a new parser using `docling` from `$PATH`.
    pub fn new() -> Self {
        Self {
            binary: "docling".to_string(),
            no_ocr: false,
        }
    }

    /// Create a parser with a specific binary path.
    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            no_ocr: false,
        }
    }

    /// Disable OCR processing (faster for programmatic PDFs).
    pub fn with_no_ocr(mut self, no_ocr: bool) -> Self {
        self.no_ocr = no_ocr;
        self
    }

    /// Invoke the Docling CLI and capture JSON output.
    fn invoke_docling(&self, input_path: &str) -> Result<DoclingDocument, ShiroError> {
        let tmpdir = tempfile::tempdir().map_err(|e| ShiroError::ParseExternal {
            message: format!("failed to create temp dir for Docling output: {e}"),
        })?;

        let mut cmd = Command::new(&self.binary);
        cmd.arg(input_path)
            .arg("--to")
            .arg("json")
            .arg("--output")
            .arg(tmpdir.path());

        if self.no_ocr {
            cmd.arg("--no-ocr");
        }

        // Suppress Docling's own stdout noise.
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        tracing::debug!(binary = %self.binary, input = %input_path, "invoking docling");

        let output = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ShiroError::ParseExternal {
                    message: format!(
                        "docling binary '{}' not found. Install with: pip install docling",
                        self.binary
                    ),
                }
            } else {
                ShiroError::ParseExternal {
                    message: format!("failed to execute docling: {e}"),
                }
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ShiroError::ParseExternal {
                message: format!(
                    "docling exited with status {}: {}",
                    output.status,
                    stderr.trim()
                ),
            });
        }

        // Find the JSON output file. Docling writes <stem>.json in the output dir.
        let json_path =
            find_json_output(tmpdir.path()).ok_or_else(|| ShiroError::ParseExternal {
                message: "docling produced no JSON output file".to_string(),
            })?;

        let json_bytes = std::fs::read(&json_path).map_err(|e| ShiroError::ParseExternal {
            message: format!(
                "failed to read Docling output at {}: {e}",
                json_path.display()
            ),
        })?;

        let docling_doc: DoclingDocument =
            serde_json::from_slice(&json_bytes).map_err(|e| ShiroError::ParseExternal {
                message: format!("failed to parse Docling JSON: {e}"),
            })?;

        // Validate schema.
        if let Some(ref name) = docling_doc.schema_name {
            if name != "DoclingDocument" {
                return Err(ShiroError::ParseExternal {
                    message: format!(
                        "unexpected Docling schema_name: '{name}', expected 'DoclingDocument'"
                    ),
                });
            }
        }

        Ok(docling_doc)
    }
}

impl Default for DoclingParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for DoclingParser {
    fn name(&self) -> &str {
        "docling"
    }

    fn version(&self) -> u32 {
        ADAPTER_VERSION
    }

    fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError> {
        // Docling needs a file on disk. If the source is already a path, use it.
        // Otherwise write to a temp file.
        let (docling_doc, _tmpfile) = if std::path::Path::new(source_uri).exists() {
            let doc = self.invoke_docling(source_uri)?;
            (doc, None)
        } else {
            // Write content to a temp file (preserve extension if possible).
            let ext = source_uri
                .rsplit('.')
                .next()
                .filter(|e| e.len() <= 10)
                .unwrap_or("pdf");
            let tmpfile = tempfile::Builder::new()
                .suffix(&format!(".{ext}"))
                .tempfile()
                .map_err(|e| ShiroError::ParseExternal {
                    message: format!("failed to create temp input file: {e}"),
                })?;
            std::io::Write::write_all(&mut &tmpfile, content).map_err(|e| {
                ShiroError::ParseExternal {
                    message: format!("failed to write temp input: {e}"),
                }
            })?;
            let doc = self.invoke_docling(tmpfile.path().to_str().unwrap_or("input.pdf"))?;
            (doc, Some(tmpfile))
        };

        let document = translate::translate(&docling_doc, source_uri, content);

        // Validate the generated IR.
        let violations = document.blocks.validate(document.canonical_text.len());
        if !violations.is_empty() {
            tracing::warn!(
                violations = ?violations,
                "Docling translation produced IR violations"
            );
            return Err(ShiroError::InvalidIr {
                message: format!(
                    "Docling translation produced {} IR violation(s): {}",
                    violations.len(),
                    violations
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            });
        }

        Ok(document)
    }
}

/// Find the first `.json` file in a directory (Docling output).
fn find_json_output(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_identity() {
        let parser = DoclingParser::new();
        assert_eq!(parser.name(), "docling");
        assert_eq!(parser.version(), ADAPTER_VERSION);
    }

    #[test]
    fn parser_missing_binary() {
        let parser = DoclingParser::with_binary("/nonexistent/docling-not-here");
        let result = parser.parse("test.pdf", b"fake");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not found") || err.to_string().contains("failed to execute"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn with_no_ocr_builder() {
        let parser = DoclingParser::new().with_no_ocr(true);
        assert!(parser.no_ocr);
    }

    #[test]
    fn default_parser() {
        let parser = DoclingParser::default();
        assert_eq!(parser.binary, "docling");
        assert!(!parser.no_ocr);
    }
}
