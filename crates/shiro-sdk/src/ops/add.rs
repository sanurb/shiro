//! `add` — stage and index a single document.

use serde::{Deserialize, Serialize};
use shiro_core::ports::Parser;
use shiro_core::ShiroError;
use shiro_index::FtsIndex;
use shiro_store::Store;

use super::document_ingestion::ingest_document_bytes;

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
    let result = ingest_document_bytes(store, fts, parser, &input.path, &content)?;

    Ok(AddOutput {
        doc_id: result.doc_id.as_str().to_string(),
        status: "READY".to_string(),
        title: result.title,
        segments: result.segments.len(),
        changed: result.changed,
    })
}
