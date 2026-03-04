//! `shiro add` — stage and process a single document.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::manifest::DocState;
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_parse::{segment_document, PlainTextParser};
use shiro_store::Store;

pub fn run(home: &ShiroHome, path: &str) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let fts = FtsIndex::open(&home.tantivy_dir())?;
    let parser = PlainTextParser;

    // Read the source file.
    let content = std::fs::read(path)?;
    let source_uri = path.to_string();

    // Parse into Document.
    use shiro_core::ports::Parser;
    let doc = parser.parse(&source_uri, &content)?;

    // Check for duplicates (content-addressed).
    if store.exists(&doc.id)? {
        let (existing, state) = store.get_document(&doc.id)?;
        let result = serde_json::json!({
            "doc_id": existing.id.as_str(),
            "status": state.as_str(),
            "title": existing.metadata.title,
            "changed": false,
        });
        return Ok(CmdOutput {
            result,
            next_actions: read_next_actions(&existing.id),
        });
    }

    // Stage document.
    store.put_document(&doc, DocState::Staged)?;
    tracing::info!(doc_id = %doc.id, "staged document");

    // Transition to INDEXING.
    store.set_state(&doc.id, DocState::Indexing)?;

    // Segment the document.
    let segments = segment_document(&doc)?;
    store.put_segments(&segments)?;
    tracing::info!(doc_id = %doc.id, segments = segments.len(), "segmented");

    // Index in Tantivy.
    fts.index_segments(&segments)?;
    tracing::info!(doc_id = %doc.id, "indexed in FTS");

    // Transition to READY.
    store.set_state(&doc.id, DocState::Ready)?;
    tracing::info!(doc_id = %doc.id, "document READY");

    let result = serde_json::json!({
        "doc_id": doc.id.as_str(),
        "status": "READY",
        "title": doc.metadata.title,
        "segments": segments.len(),
        "changed": true,
    });

    Ok(CmdOutput {
        result,
        next_actions: read_next_actions(&doc.id),
    })
}

fn read_next_actions(doc_id: &shiro_core::DocId) -> Vec<NextAction> {
    let mut params = BTreeMap::new();
    params.insert(
        "doc_id".to_string(),
        ParamMeta {
            value: Some(serde_json::json!(doc_id.as_str())),
            default: None,
            description: Some("Document ID".to_string()),
        },
    );

    vec![
        NextAction::with_params(
            "shiro read <doc_id> --text",
            "Read document text",
            params.clone(),
        ),
        NextAction::with_params(
            "shiro read <doc_id> --blocks",
            "View document blocks",
            params,
        ),
        NextAction::simple("shiro search <query>", "Search the library"),
    ]
}
