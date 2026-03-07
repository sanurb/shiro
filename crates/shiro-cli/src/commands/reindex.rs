//! `shiro reindex` — rebuild FTS index from stored segments.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let output = shiro_sdk::ops::reindex::execute(home, &store)?;

    let result = serde_json::json!({
        "actions": [serde_json::json!({
            "index": output.index,
            "status": output.status,
            "documents": output.documents,
            "segments": output.segments,
            "generation": output.generation,
        })]
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro doctor", "Verify index health"),
            NextAction::simple("shiro search <query>", "Search documents"),
        ],
    })
}
