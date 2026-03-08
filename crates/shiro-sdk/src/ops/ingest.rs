//! `ingest` — batch-add documents from directories.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::manifest::DocState;
use shiro_core::ports::Parser;
use shiro_core::{ErrorCode, ShiroError};
use shiro_index::FtsIndex;
use shiro_parse::{segment_document, SEGMENTER_VERSION};
use shiro_store::Store;

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
    let mut all_segments = Vec::new();

    // Phase 1: parse files and write docs to SQLite in a single transaction.
    store.begin()?;
    for file_path in &files {
        match parse_and_store(store, parser, file_path) {
            Ok(Some(segments)) => {
                emit(&IngestEvent::Indexed {
                    path: file_path.clone(),
                    doc_id: segments
                        .first()
                        .map(|s| s.doc_id.as_str().to_string())
                        .unwrap_or_default(),
                    segments: segments.len(),
                });
                all_segments.extend(segments);
                added += 1;
                ready += 1;
            }
            Ok(None) => {
                emit(&IngestEvent::Skipped {
                    path: file_path.clone(),
                    reason: "already_exists".to_string(),
                });
                ready += 1;
            }
            Err(e) => {
                let code = ErrorCode::from_error(&e);
                emit(&IngestEvent::Failed {
                    path: file_path.clone(),
                    error: e.to_string(),
                });
                failures.push(IngestFailure {
                    source: file_path.clone(),
                    code: code.as_str().to_string(),
                    message: e.to_string(),
                });
                failed += 1;
                tracing::warn!(path = %file_path, error = %e, "ingest failed");
            }
        }
    }
    store.commit()?;

    // Phase 2: index all segments in a single Tantivy writer+commit.
    if !all_segments.is_empty() {
        fts.index_segments(&all_segments)?;
    }

    // Phase 3: mark all added docs as READY in one transaction.
    if added > 0 {
        store.begin()?;
        let mut seen = HashSet::new();
        for seg in &all_segments {
            if seen.insert(seg.doc_id.clone()) {
                store.set_state(&seg.doc_id, DocState::Ready)?;
            }
        }
        store.commit()?;
    }

    emit(&IngestEvent::Complete {
        added,
        ready,
        failed,
    });

    Ok(IngestOutput {
        added,
        ready,
        failed,
        failures,
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Parse a file and store its document + segments. Returns segments if new.
fn parse_and_store(
    store: &Store,
    parser: &dyn Parser,
    path: &str,
) -> Result<Option<Vec<shiro_core::ir::Segment>>, ShiroError> {
    let content = std::fs::read(path)?;
    let doc = parser.parse(path, &content)?;

    if store.exists(&doc.id)? {
        return Ok(None);
    }

    store.put_document(&doc, DocState::Indexing)?;

    // Persist processing fingerprint (ADR-004) for staleness detection.
    let fingerprint =
        ProcessingFingerprint::new(parser.name(), parser.version(), SEGMENTER_VERSION);
    store.set_fingerprint(&doc.id, &fingerprint)?;

    let segments = segment_document(&doc)?;
    store.put_segments(&segments)?;

    tracing::info!(doc_id = %doc.id, path = %path, "ingested");
    Ok(Some(segments))
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
        Some("txt" | "md" | "markdown")
    )
}
