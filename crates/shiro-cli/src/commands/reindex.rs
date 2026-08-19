//! `shiro reindex` — rebuild FTS index from stored segments.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let engine = crate::runtime::open_engine_for_reindex(home)?;
    let actions = engine
        .reindex_all()?
        .into_iter()
        .map(|output| {
            serde_json::json!({
                "index": output.index,
                "status": output.status,
                "documents": output.documents,
                "segments": output.segments,
                "generation": output.generation,
            })
        })
        .collect::<Vec<_>>();

    let result = serde_json::json!({ "actions": actions });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro doctor", "Verify index health"),
            NextAction::simple("shiro search <query>", "Search documents"),
        ],
    })
}
