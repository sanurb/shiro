//! `shiro enrich` — run enrichment on a document.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

pub fn run(home: &ShiroHome, doc_id_str: &str) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let input = shiro_sdk::ops::enrich::EnrichInput {
        doc_id: doc_id_str.to_string(),
    };
    let output = shiro_sdk::ops::enrich::execute(&store, &input)?;

    let result = serde_json::json!({
        "doc_id": output.doc_id,
        "provider": "heuristic",
        "title": output.title,
        "summary_length": output.summary_length,
        "tags": output.tags,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple(format!("shiro read {}", output.doc_id), "Read the document"),
            NextAction::simple("shiro search <query>", "Search documents"),
        ],
    })
}
