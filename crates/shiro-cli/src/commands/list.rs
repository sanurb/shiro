//! `shiro list` — list documents in the library.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

pub fn run(home: &ShiroHome, limit: usize) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;

    // Fetch limit+1 to detect truncation.
    let docs = store.list_documents(limit + 1)?;
    let truncated = docs.len() > limit;
    let showing = if truncated { &docs[..limit] } else { &docs };

    let items: Vec<serde_json::Value> = showing
        .iter()
        .map(|(doc_id, state, title)| {
            serde_json::json!({
                "doc_id": doc_id.as_str(),
                "status": state.as_str(),
                "title": title,
            })
        })
        .collect();

    let total = docs.len();
    let result = serde_json::json!({
        "items": items,
        "showing": showing.len(),
        "total": total,
        "truncated": truncated,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro add <path>", "Add a document"),
            NextAction::simple("shiro search <query>", "Search the library"),
        ],
    })
}
