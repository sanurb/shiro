//! `shiro remove` — tombstone or purge a document.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::manifest::DocState;
use shiro_core::{DocId, ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

pub fn run(home: &ShiroHome, id_or_title: &str, purge: bool) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let doc_id = resolve_doc_id(&store, id_or_title)?;

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

fn resolve_doc_id(store: &Store, id_or_title: &str) -> Result<DocId, ShiroError> {
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
