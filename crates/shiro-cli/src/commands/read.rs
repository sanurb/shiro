//! `shiro read` — fetch document content by ID or title.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::ops::read::{self, ReadContent, ReadInput};
use shiro_store::Store;

pub use shiro_sdk::ReadMode;

pub fn run(home: &ShiroHome, id_or_title: &str, mode: ReadMode) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;
    let input = ReadInput {
        id: id_or_title.to_string(),
        mode,
    };
    let output = read::execute(&store, &input)?;

    // Flatten content into result to match CLI output contract.
    let mut result = serde_json::json!({
        "doc_id": output.doc_id,
        "title": output.title,
        "state": output.state,
    });

    match &output.content {
        ReadContent::Text { text, truncated } => {
            result["text"] = serde_json::json!(text);
            result["truncated"] = serde_json::json!(truncated);
        }
        ReadContent::Blocks { blocks } => {
            result["blocks"] = serde_json::to_value(blocks).unwrap_or_default();
        }
        ReadContent::Outline { lines } => {
            result["lines"] = serde_json::to_value(lines).unwrap_or_default();
        }
    }

    let mut params = BTreeMap::new();
    params.insert(
        "doc_id".to_string(),
        ParamMeta {
            value: Some(serde_json::json!(output.doc_id)),
            default: None,
            description: Some("Document ID".to_string()),
        },
    );

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro search <query>", "Search the library"),
            NextAction::with_params("shiro remove <doc_id>", "Remove this document", params),
        ],
    })
}
