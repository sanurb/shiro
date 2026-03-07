use shiro_core::{DocId, ShiroError};
use shiro_store::Store;

pub mod add;
pub mod capabilities;
pub mod completions;
pub mod config;
pub mod doctor;
pub mod enrich;
pub mod explain;
pub mod ingest;
pub mod init;
pub mod list;
pub mod mcp;
pub mod read;
pub mod reindex;
pub mod remove;
pub mod root;
pub mod search;
pub mod taxonomy;

/// Resolve a doc ID from either a raw `doc_*` string or a title search.
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
