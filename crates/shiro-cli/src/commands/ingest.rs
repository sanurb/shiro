//! `shiro ingest` — batch-add documents from directories.

use std::path::Path;

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::manifest::DocState;
use shiro_core::ports::Parser;
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_parse::{segment_document, PlainTextParser};
use shiro_store::Store;

pub fn run(
    home: &ShiroHome,
    dirs: &[std::path::PathBuf],
    glob: Option<&str>,
    max_files: Option<usize>,
) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let fts = FtsIndex::open(&home.tantivy_dir())?;
    let parser = PlainTextParser;

    // Collect files from directories (deterministic: sorted).
    let pattern = glob.unwrap_or("*.{txt,md}");
    let mut files = Vec::new();
    for dir in dirs {
        collect_files(dir, pattern, &mut files)?;
    }
    files.sort();
    if let Some(max) = max_files {
        files.truncate(max);
    }

    let mut added = 0usize;
    let mut ready = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();
    let mut all_segments = Vec::new();

    // Phase 1: parse files and write docs to SQLite in a single transaction.
    store.begin()?;
    for file_path in &files {
        match parse_and_store(&store, &parser, file_path) {
            Ok(Some(segments)) => {
                all_segments.extend(segments);
                added += 1;
                ready += 1;
            }
            Ok(None) => {
                // Already existed, skip.
                ready += 1;
            }
            Err(e) => {
                let code = shiro_core::ErrorCode::from_error(&e);
                failures.push(serde_json::json!({
                    "source": file_path,
                    "code": code.as_str(),
                    "message": e.to_string(),
                }));
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
        let mut seen = std::collections::HashSet::new();
        for seg in &all_segments {
            if seen.insert(seg.doc_id.clone()) {
                store.set_state(&seg.doc_id, DocState::Ready)?;
            }
        }
        store.commit()?;
    }

    let result = serde_json::json!({
        "added": added,
        "ready": ready,
        "failed": failed,
        "failures": failures,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro list", "List documents"),
            NextAction::simple("shiro search <query>", "Search the library"),
        ],
    })
}

/// Parse a file and store its document + segments. Returns segments if new.
fn parse_and_store(
    store: &Store,
    parser: &PlainTextParser,
    path: &str,
) -> Result<Option<Vec<shiro_core::ir::Segment>>, ShiroError> {
    let content = std::fs::read(path)?;
    let doc = parser.parse(path, &content)?;

    if store.exists(&doc.id)? {
        return Ok(None);
    }

    store.put_document(&doc, DocState::Indexing)?;
    let segments = segment_document(&doc)?;
    store.put_segments(&segments)?;

    tracing::info!(doc_id = %doc.id, path = %path, "ingested");
    Ok(Some(segments))
}

/// Collect files matching a glob pattern from a directory.
///
/// For the initial implementation, we do a simple recursive file walk
/// filtering by extension. Full glob support is a future enhancement.
fn collect_files(dir: &Path, _pattern: &str, out: &mut Vec<String>) -> Result<(), ShiroError> {
    if !dir.is_dir() {
        return Err(ShiroError::InvalidInput {
            message: format!("not a directory: {}", dir.display()),
        });
    }

    walk_dir(dir, out)?;
    Ok(())
}

fn walk_dir(dir: &std::path::Path, out: &mut Vec<String>) -> Result<(), ShiroError> {
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

fn is_supported(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("txt" | "md" | "markdown")
    )
}
