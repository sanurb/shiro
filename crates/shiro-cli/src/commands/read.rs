//! `shiro read` — fetch document content by ID or title.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

#[derive(Debug, Clone, Copy)]
pub enum ReadMode {
    Text,
    Blocks,
    Outline,
}

pub fn run(home: &ShiroHome, id_or_title: &str, mode: ReadMode) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let doc_id = super::resolve_doc_id(&store, id_or_title)?;
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
