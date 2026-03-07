//! `read` — fetch document content by ID or title.

use serde::{Deserialize, Serialize};
use shiro_core::{DocId, ShiroError};
use shiro_store::Store;

/// Maximum characters returned in Text mode before truncation.
const TEXT_LIMIT: usize = 50_000;

// ---------------------------------------------------------------------------
// Shared helper
// ---------------------------------------------------------------------------

/// Resolve an opaque `id_or_title` string to a concrete [`DocId`].
///
/// Tries `DocId::from_stored` first (exact ID match), then falls back to a
/// linear scan of document titles.
pub(crate) fn resolve_doc_id(store: &Store, id_or_title: &str) -> Result<DocId, ShiroError> {
    if id_or_title.starts_with("doc_") {
        if let Ok(id) = DocId::from_stored(id_or_title) {
            if store.exists(&id)? {
                return Ok(id);
            }
        }
    }
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

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ReadMode {
    Text,
    Blocks,
    Outline,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadInput {
    pub id: String,
    pub mode: ReadMode,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadOutput {
    pub doc_id: String,
    pub title: Option<String>,
    pub state: String,
    pub content: ReadContent,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadContent {
    Text { text: String, truncated: bool },
    Blocks { blocks: Vec<BlockInfo> },
    Outline { lines: Vec<String> },
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlockInfo {
    pub index: usize,
    pub kind: String,
    pub span_start: usize,
    pub span_end: usize,
    pub body: String,
}

// ---------------------------------------------------------------------------
// Execute
// ---------------------------------------------------------------------------

pub fn execute(store: &Store, input: &ReadInput) -> Result<ReadOutput, ShiroError> {
    let doc_id = resolve_doc_id(store, &input.id)?;
    let (doc, state) = store.get_document(&doc_id)?;

    let content = match input.mode {
        ReadMode::Text => {
            let text = &doc.canonical_text;
            let truncated = text.len() > TEXT_LIMIT;
            let showing = if truncated {
                text[..TEXT_LIMIT].to_string()
            } else {
                text.clone()
            };
            ReadContent::Text {
                text: showing,
                truncated,
            }
        }
        ReadMode::Blocks => {
            let graph = &doc.blocks;
            if graph.blocks.is_empty() {
                // Fallback for pre-v5 documents without persisted graph.
                let segments = store.get_segments(&doc.id)?;
                let blocks = segments
                    .iter()
                    .map(|s| BlockInfo {
                        index: s.index,
                        kind: "segment".to_string(),
                        span_start: s.span.start(),
                        span_end: s.span.end(),
                        body: s.body.clone(),
                    })
                    .collect();
                ReadContent::Blocks { blocks }
            } else {
                let blocks = graph
                    .reading_order
                    .iter()
                    .enumerate()
                    .filter_map(|(pos, idx)| {
                        graph.blocks.get(idx.0).map(|block| BlockInfo {
                            index: pos,
                            kind: format!("{:?}", block.kind).to_lowercase(),
                            span_start: block.span.start(),
                            span_end: block.span.end(),
                            body: block.canonical_text.clone(),
                        })
                    })
                    .collect();
                ReadContent::Blocks { blocks }
            }
        }
        ReadMode::Outline => {
            let segments = store.get_segments(&doc.id)?;
            let lines = segments
                .iter()
                .map(|s| {
                    let first_line = s.body.lines().next().unwrap_or("");
                    format!("[{}] {first_line}", s.index)
                })
                .collect();
            ReadContent::Outline { lines }
        }
    };

    Ok(ReadOutput {
        doc_id: doc.id.as_str().to_string(),
        title: doc.metadata.title.clone(),
        state: state.as_str().to_string(),
        content,
    })
}
