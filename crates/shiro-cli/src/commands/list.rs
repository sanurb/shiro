//! `shiro list` — list documents in the library.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

pub fn run(home: &ShiroHome, limit: usize) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let input = shiro_sdk::ops::list::ListInput { limit };
    let output = shiro_sdk::ops::list::execute(&store, &input)?;

    let result = serde_json::json!({
        "items": output.documents,
        "showing": output.documents.len(),
        "truncated": output.truncated,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro add <path>", "Add a document"),
            NextAction::simple("shiro search <query>", "Search the library"),
        ],
    })
}
