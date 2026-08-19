//! `list` — enumerate documents in the store.

use serde::{Deserialize, Serialize};
use shiro_core::ShiroError;
use shiro_store::Store;

use super::search::SearchFilters;
use crate::retrieval_policy::ResolvedSearchFilters;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListInput {
    pub limit: usize,
    #[serde(default)]
    pub filters: SearchFilters,
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
    let filters = ResolvedSearchFilters::resolve(store, &input.filters)?;
    let docs = store.list_all_documents()?;
    let mut filtered = Vec::new();
    for document in docs {
        if filters.matches_document(store, &document.0)? {
            filtered.push(document);
        }
    }
    let truncated = filtered.len() > input.limit;

    let documents: Vec<ListEntry> = filtered
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
