//! `read` — fetch document content by ID or title.

use serde::{Deserialize, Serialize};
use shiro_core::{evidence_handle_for_block, DocId, EvidenceHandleId, ShiroError};
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

/// Canonical document, block, outline, or one-based source-page read mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ReadMode {
    Text,
    Blocks,
    Outline,
    Page,
}

/// Deferred evidence read request by document/title or stable `blk_` handle.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadInput {
    pub id: String,
    pub mode: ReadMode,
    #[serde(default)]
    pub page: Option<u32>,
}

/// Canonical deferred evidence with explicit stable-handle resolution metadata.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadOutput {
    pub doc_id: String,
    pub title: Option<String>,
    pub state: String,
    pub evidence_resolution: Option<EvidenceResolution>,
    pub content: ReadContent,
}

/// Active or superseded status for the exact requested evidence handle; redirects are never implicit.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EvidenceResolution {
    pub requested_handle: EvidenceHandleId,
    pub status: String,
    pub superseded_by: Option<EvidenceHandleId>,
}

/// Source-faithful text, canonical blocks, or outline content returned by a deferred read.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadContent {
    Text { text: String, truncated: bool },
    Blocks { blocks: Vec<BlockInfo> },
    Outline { lines: Vec<String> },
}

/// Canonical block evidence with stable identity and parser-neutral source locations.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlockInfo {
    pub evidence_handle: Option<EvidenceHandleId>,
    pub index: usize,
    pub kind: String,
    /// One-based heading depth; absent for non-headings or unknown parser depth.
    pub heading_level: Option<u32>,
    pub span_start: usize,
    pub span_end: usize,
    pub body: String,
    pub source_locators: Vec<shiro_core::SourceLocator>,
}

// ---------------------------------------------------------------------------
// Execute
// ---------------------------------------------------------------------------

/// Resolve and read source-faithful evidence without exposing operational segment identities.
pub fn execute(store: &Store, input: &ReadInput) -> Result<ReadOutput, ShiroError> {
    if input.id.starts_with("blk_") {
        let handle = EvidenceHandleId::from_stored(&input.id)
            .map_err(|message| ShiroError::InvalidInput { message })?;
        let resolved = store.get_evidence_handle(&handle)?;
        let (document, state) = store.get_document(&resolved.doc_id)?;
        if state == shiro_core::DocState::Deleted {
            return Err(ShiroError::NotFoundMsg {
                message: format!("evidence handle not found: {handle}"),
            });
        }
        return Ok(ReadOutput {
            doc_id: resolved.doc_id.as_str().to_string(),
            title: document.metadata.title,
            state: state.as_str().to_string(),
            evidence_resolution: Some(EvidenceResolution {
                requested_handle: resolved.handle_id.clone(),
                status: resolved.status,
                superseded_by: resolved.superseded_by,
            }),
            content: ReadContent::Blocks {
                blocks: vec![BlockInfo {
                    evidence_handle: Some(resolved.handle_id),
                    index: resolved.block_idx,
                    kind: format!("{:?}", resolved.block_kind).to_lowercase(),
                    heading_level: resolved.heading_level,
                    span_start: resolved.span.start(),
                    span_end: resolved.span.end(),
                    body: resolved.canonical_text,
                    source_locators: resolved.source_locators,
                }],
            },
        });
    }

    let doc_id = resolve_doc_id(store, &input.id)?;
    let (doc, state) = store.get_document(&doc_id)?;

    let content =
        match input.mode {
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
                            evidence_handle: None,
                            index: s.index,
                            kind: "segment".to_string(),
                            heading_level: None,
                            span_start: s.span.start(),
                            span_end: s.span.end(),
                            body: s.body.clone(),
                            source_locators: Vec::new(),
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
                                evidence_handle: evidence_handle_for_block(&doc.id, graph, idx.0),
                                index: pos,
                                kind: format!("{:?}", block.kind).to_lowercase(),
                                heading_level: block
                                    .heading_level
                                    .map(shiro_core::DocumentHeadingLevel::as_u32),
                                span_start: block.span.start(),
                                span_end: block.span.end(),
                                body: block.canonical_text.clone(),
                                source_locators: block.source_locators.clone(),
                            })
                        })
                        .collect();
                    ReadContent::Blocks { blocks }
                }
            }
            ReadMode::Page => {
                let page_number = input.page.filter(|page| *page > 0).ok_or_else(|| {
                    ShiroError::InvalidInput {
                        message: "page reads require a one-based page number".to_string(),
                    }
                })?;
                let blocks = doc
                    .blocks
                    .reading_order
                    .iter()
                    .filter_map(|index| {
                        let block = doc.blocks.blocks.get(index.0)?;
                        block
                            .source_locators
                            .iter()
                            .any(|locator| locator.page_number() == page_number)
                            .then(|| BlockInfo {
                                evidence_handle: evidence_handle_for_block(
                                    &doc.id,
                                    &doc.blocks,
                                    index.0,
                                ),
                                index: index.0,
                                kind: format!("{:?}", block.kind).to_lowercase(),
                                heading_level: block
                                    .heading_level
                                    .map(shiro_core::DocumentHeadingLevel::as_u32),
                                span_start: block.span.start(),
                                span_end: block.span.end(),
                                body: block.canonical_text.clone(),
                                source_locators: block.source_locators.clone(),
                            })
                    })
                    .collect::<Vec<_>>();
                if blocks.is_empty() {
                    return Err(ShiroError::NotFoundMsg {
                        message: format!("page {page_number} not found for {}", doc.id),
                    });
                }
                ReadContent::Blocks { blocks }
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
        evidence_resolution: None,
        content,
    })
}
