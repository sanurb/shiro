//! `ingest` — batch-add documents from directories.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shiro_core::ports::Parser;
use shiro_core::{ErrorCode, ShiroError};
use shiro_index::FtsIndex;
use shiro_store::Store;

use super::document_ingestion::{publish_staged_documents, stage_document_bytes};

// ── Inputs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IngestInput {
    pub dirs: Vec<String>,
    pub max_files: Option<usize>,
}

// ── Outputs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IngestFailure {
    pub source: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IngestOutput {
    pub added: usize,
    pub ready: usize,
    pub failed: usize,
    pub failures: Vec<IngestFailure>,
    /// Automatic concept proposals created for newly ingested documents.
    pub concept_proposals: Vec<super::model_enrichment::ModelEnrichmentProposalOutput>,
    /// Internal identities used by the Engine to run post-publication proposal policy.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) ingested_document_ids: Vec<shiro_core::DocId>,
}

// ── Progress events ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum IngestEvent {
    Start {
        total_files: usize,
    },
    Indexed {
        path: String,
        doc_id: String,
        segments: usize,
    },
    Skipped {
        path: String,
        reason: String,
    },
    Failed {
        path: String,
        error: String,
    },
    Complete {
        added: usize,
        ready: usize,
        failed: usize,
    },
}

// ── Execute ─────────────────────────────────────────────────────────────────

pub fn execute(
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    input: &IngestInput,
    on_event: Option<&dyn Fn(&IngestEvent)>,
) -> Result<IngestOutput, ShiroError> {
    let emit = |evt: &IngestEvent| {
        if let Some(cb) = on_event {
            cb(evt);
        }
    };

    // Collect files from directories (deterministic: sorted).
    let mut files: Vec<String> = Vec::new();
    for dir in &input.dirs {
        collect_files(Path::new(dir), &mut files)?;
    }
    files.sort();
    if let Some(max) = input.max_files {
        files.truncate(max);
    }

    emit(&IngestEvent::Start {
        total_files: files.len(),
    });

    let mut added = 0usize;
    let mut ready = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();
    let mut staged = Vec::new();
    let mut staged_doc_ids = HashSet::new();

    for file_path in &files {
        let result = std::fs::read(file_path)
            .map_err(ShiroError::from)
            .and_then(|content| stage_document_bytes(store, parser, file_path, &content));

        match result {
            Ok(document) if document.changed && staged_doc_ids.insert(document.doc_id.clone()) => {
                staged.push((file_path.clone(), document));
            }
            Ok(_) => {
                emit(&IngestEvent::Skipped {
                    path: file_path.clone(),
                    reason: "already_exists".to_string(),
                });
                ready += 1;
            }
            Err(error) => {
                record_ingest_failure(&emit, file_path, &error, &mut failures, &mut failed);
            }
        }
    }

    let staged_refs: Vec<_> = staged.iter().map(|(_, document)| document).collect();
    match publish_staged_documents(store, fts, &staged_refs) {
        Ok(()) => {
            for (file_path, document) in &staged {
                emit(&IngestEvent::Indexed {
                    path: file_path.clone(),
                    doc_id: document.doc_id.as_str().to_string(),
                    segments: document.segments.len(),
                });
                added += 1;
                ready += 1;
            }
        }
        Err(error) => {
            for (file_path, _) in &staged {
                record_ingest_failure(&emit, file_path, &error, &mut failures, &mut failed);
            }
        }
    }

    emit(&IngestEvent::Complete {
        added,
        ready,
        failed,
    });

    let ingested_document_ids = staged
        .iter()
        .take(added)
        .map(|(_, document)| document.doc_id.clone())
        .collect();
    Ok(IngestOutput {
        added,
        ready,
        failed,
        failures,
        concept_proposals: Vec::new(),
        ingested_document_ids,
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn record_ingest_failure(
    emit: &impl Fn(&IngestEvent),
    file_path: &str,
    error: &ShiroError,
    failures: &mut Vec<IngestFailure>,
    failed: &mut usize,
) {
    let code = ErrorCode::from_error(error);
    emit(&IngestEvent::Failed {
        path: file_path.to_string(),
        error: error.to_string(),
    });
    failures.push(IngestFailure {
        source: file_path.to_string(),
        code: code.as_str().to_string(),
        message: error.to_string(),
    });
    *failed += 1;
    tracing::warn!(path = %file_path, error = %error, "ingest failed");
}

/// Collect supported files from a directory via recursive walk.
fn collect_files(dir: &Path, out: &mut Vec<String>) -> Result<(), ShiroError> {
    if !dir.is_dir() {
        return Err(ShiroError::InvalidInput {
            message: format!("not a directory: {}", dir.display()),
        });
    }
    walk_dir(dir, out)?;
    Ok(())
}

fn walk_dir(dir: &Path, out: &mut Vec<String>) -> Result<(), ShiroError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if is_supported(&path) {
            if let Some(s) = path.to_str() {
                out.push(s.to_string());
            }
        }
    }
    Ok(())
}

fn is_supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("txt" | "md" | "markdown" | "pdf")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::ir::Document;
    use shiro_parse::PlainTextParser;

    struct SelectiveFailureParser;

    impl Parser for SelectiveFailureParser {
        fn name(&self) -> &str {
            "selective_failure"
        }

        fn version(&self) -> u32 {
            1
        }

        fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError> {
            if source_uri.ends_with("a_bad.txt") {
                return Err(ShiroError::ParseMd {
                    message: "selective ingestion test parse failure".to_string(),
                });
            }
            PlainTextParser.parse(source_uri, content)
        }
    }

    #[test]
    fn batch_ingestion_continues_after_document_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap();
        let corpus = root.join("corpus");
        std::fs::create_dir_all(corpus.as_std_path()).unwrap();
        std::fs::write(corpus.join("a_bad.txt").as_std_path(), b"bad content").unwrap();
        std::fs::write(
            corpus.join("b_good.txt").as_std_path(),
            b"successful searchable content",
        )
        .unwrap();

        let store = Store::open(&root.join("shiro.db")).unwrap();
        let fts = FtsIndex::open(&root.join("tantivy")).unwrap();
        let input = IngestInput {
            dirs: vec![corpus.as_str().to_string()],
            max_files: None,
        };

        let output = execute(&store, &fts, &SelectiveFailureParser, &input, None).unwrap();

        assert_eq!(output.added, 1);
        assert_eq!(output.ready, 1);
        assert_eq!(output.failed, 1);
        assert_eq!(output.failures.len(), 1);
        assert_eq!(output.failures[0].source, corpus.join("a_bad.txt").as_str());
        assert!(!fts.search("searchable", 10).unwrap().is_empty());
    }
}
