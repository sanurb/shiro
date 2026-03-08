//! `add` — stage and index a single document.

use serde::{Deserialize, Serialize};
use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::manifest::DocState;
use shiro_core::ports::Parser;
use shiro_core::ShiroError;
use shiro_index::FtsIndex;
use shiro_parse::{segment_document, SEGMENTER_VERSION};
use shiro_store::Store;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddInput {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddOutput {
    pub doc_id: String,
    pub status: String,
    pub title: Option<String>,
    pub segments: usize,
    pub changed: bool,
}

pub fn execute(
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    input: &AddInput,
) -> Result<AddOutput, ShiroError> {
    let content = std::fs::read(&input.path)?;
    let doc = parser.parse(&input.path, &content)?;

    // Content-addressed dedup: if the document already exists, return existing state.
    if store.exists(&doc.id)? {
        let (existing, state) = store.get_document(&doc.id)?;
        return Ok(AddOutput {
            doc_id: existing.id.as_str().to_string(),
            status: state.as_str().to_string(),
            title: existing.metadata.title.clone(),
            segments: 0,
            changed: false,
        });
    }

    // Stage → Index → Ready pipeline.
    store.put_document(&doc, DocState::Staged)?;

    // Persist processing fingerprint (ADR-004) for staleness detection.
    let fingerprint = ProcessingFingerprint::new(parser.name(), parser.version(), SEGMENTER_VERSION);
    store.set_fingerprint(&doc.id, &fingerprint)?;
    tracing::info!(doc_id = %doc.id, "staged document");

    store.set_state(&doc.id, DocState::Indexing)?;

    let segments = segment_document(&doc)?;
    store.put_segments(&segments)?;
    tracing::info!(doc_id = %doc.id, segments = segments.len(), "segmented");

    fts.index_segments(&segments)?;
    tracing::info!(doc_id = %doc.id, "indexed in FTS");

    store.set_state(&doc.id, DocState::Ready)?;
    tracing::info!(doc_id = %doc.id, "document READY");

    Ok(AddOutput {
        doc_id: doc.id.as_str().to_string(),
        status: "READY".to_string(),
        title: doc.metadata.title.clone(),
        segments: segments.len(),
        changed: true,
    })
}
