//! `remove` — tombstone or purge a document.

use serde::{Deserialize, Serialize};
use shiro_core::manifest::DocState;
use shiro_core::ShiroError;
use shiro_index::FtsIndex;
use shiro_store::Store;

use super::read::resolve_doc_id;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoveInput {
    pub id: String,
    pub purge: bool,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoveOutput {
    pub doc_id: String,
    pub previous_state: String,
}

pub fn execute(
    store: &Store,
    fts: Option<&FtsIndex>,
    input: &RemoveInput,
) -> Result<RemoveOutput, ShiroError> {
    let doc_id = resolve_doc_id(store, &input.id)?;
    let (_doc, state) = store.get_document(&doc_id)?;
    let previous_state = state.as_str().to_string();

    // Tombstone the document.
    store.set_state(&doc_id, DocState::Deleted)?;
    tracing::info!(doc_id = %doc_id, "tombstoned document");

    if input.purge {
        store.purge_derived(&doc_id)?;
        if let Some(fts) = fts {
            fts.delete_doc(&doc_id)?;
        }
        tracing::info!(doc_id = %doc_id, "purged from FTS and store");
    }

    Ok(RemoveOutput {
        doc_id: doc_id.as_str().to_string(),
        previous_state,
    })
}
