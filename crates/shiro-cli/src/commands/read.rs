//! `shiro read` — fetch document content by ID or title.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{DocId, ShiroError, ShiroHome};
use shiro_store::Store;

#[derive(Debug, Clone, Copy)]
pub enum ReadMode {
    Text,
    Blocks,
    Outline,
}

pub fn run(home: &ShiroHome, id_or_title: &str, mode: ReadMode) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let doc_id = resolve_doc_id(&store, id_or_title)?;
    let (doc, state) = store.get_document(&doc_id)?;

    let result = match mode {
        ReadMode::Text => {
            let text = &doc.canonical_text;
            let truncated = text.len() > 50_000;
            let showing = if truncated {
                &text[..50_000]
            } else {
                text.as_str()
            };
            serde_json::json!({
                "doc_id": doc.id.as_str(),
                "title": doc.metadata.title,
                "status": state.as_str(),
                "text": showing,
                "truncated": truncated,
            })
        }
        ReadMode::Blocks => {
            let segments = store.get_segments(&doc.id)?;
            let blocks: Vec<serde_json::Value> = segments
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "segment_id": s.id.as_str(),
                        "index": s.index,
                        "span": { "start": s.span.start(), "end": s.span.end() },
                        "body": s.body,
                    })
                })
                .collect();
            serde_json::json!({
                "doc_id": doc.id.as_str(),
                "title": doc.metadata.title,
                "status": state.as_str(),
                "blocks": blocks,
                "total_blocks": blocks.len(),
            })
        }
        ReadMode::Outline => {
            // Simplified outline: list segment indices and first line of each.
            let segments = store.get_segments(&doc.id)?;
            let outline: Vec<serde_json::Value> = segments
                .iter()
                .map(|s| {
                    let first_line = s.body.lines().next().unwrap_or("");
                    serde_json::json!({
                        "index": s.index,
                        "preview": first_line,
                    })
                })
                .collect();
            serde_json::json!({
                "doc_id": doc.id.as_str(),
                "title": doc.metadata.title,
                "status": state.as_str(),
                "outline": outline,
            })
        }
    };

    let mut params = BTreeMap::new();
    params.insert(
        "doc_id".to_string(),
        ParamMeta {
            value: Some(serde_json::json!(doc.id.as_str())),
            default: None,
            description: Some("Document ID".to_string()),
        },
    );

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro search <query>", "Search the library"),
            NextAction::with_params("shiro remove <doc_id>", "Remove this document", params),
        ],
    })
}

/// Resolve a doc ID from either a raw `doc_*` string or a title search.
fn resolve_doc_id(store: &Store, id_or_title: &str) -> Result<DocId, ShiroError> {
    // If it looks like a doc_id, try direct lookup.
    if id_or_title.starts_with("doc_") {
        if let Ok(id) = DocId::from_stored(id_or_title) {
            if store.exists(&id)? {
                return Ok(id);
            }
        }
    }

    // Otherwise, search by title (first match).
    let docs = store.list_documents(1000)?;
    for (doc_id, _state, title) in &docs {
        if let Some(t) = title {
            if t == id_or_title {
                return Ok(doc_id.clone());
            }
        }
    }

    Err(ShiroError::NotFoundMsg {
        message: format!("no document matching '{id_or_title}'"),
    })
}
