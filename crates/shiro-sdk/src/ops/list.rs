//! `list` — enumerate documents in the store.

use serde::{Deserialize, Serialize};
use shiro_core::ShiroError;
use shiro_store::Store;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListInput {
    pub limit: usize,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListEntry {
    pub doc_id: String,
    pub status: String,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListOutput {
    pub documents: Vec<ListEntry>,
    pub truncated: bool,
}

pub fn execute(store: &Store, input: &ListInput) -> Result<ListOutput, ShiroError> {
    // Fetch limit+1 to detect truncation.
    let docs = store.list_documents(input.limit + 1)?;
    let truncated = docs.len() > input.limit;

    let documents: Vec<ListEntry> = docs
        .iter()
        .take(input.limit)
        .map(|(doc_id, state, title)| ListEntry {
            doc_id: doc_id.as_str().to_string(),
            status: state.as_str().to_string(),
            title: title.clone(),
        })
        .collect();

    Ok(ListOutput {
        documents,
        truncated,
    })
}
