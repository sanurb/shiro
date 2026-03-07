//! `shiro remove` — tombstone or purge a document.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

pub fn run(home: &ShiroHome, id_or_title: &str, purge: bool) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let fts = if purge {
        Some(FtsIndex::open(&home.tantivy_dir())?)
    } else {
        None
    };
    let input = shiro_sdk::ops::remove::RemoveInput {
        id: id_or_title.to_string(),
        purge,
    };
    let output = shiro_sdk::ops::remove::execute(&store, fts.as_ref(), &input)?;

    let result = serde_json::json!({
        "doc_id": output.doc_id,
        "removed": true,
        "purged": purge,
        "previous_state": output.previous_state,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![NextAction::simple("shiro list", "List remaining documents")],
    })
}
