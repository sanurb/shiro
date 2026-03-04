//! `shiro remove` — tombstone or purge a document.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::manifest::DocState;
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

pub fn run(home: &ShiroHome, id_or_title: &str, purge: bool) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let doc_id = super::resolve_doc_id(&store, id_or_title)?;

    // Tombstone the document.
    store.set_state(&doc_id, DocState::Deleted)?;
    tracing::info!(doc_id = %doc_id, "tombstoned document");

    if purge {
        // Also remove from FTS index.
        let fts = FtsIndex::open(&home.tantivy_dir())?;
        fts.delete_doc(&doc_id)?;
        tracing::info!(doc_id = %doc_id, "purged from FTS index");
    }

    let result = serde_json::json!({
        "doc_id": doc_id.as_str(),
        "removed": true,
        "purged": purge,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![NextAction::simple("shiro list", "List remaining documents")],
    })
}
