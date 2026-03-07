//! `shiro add` — stage and process a single document.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{AddInput, Engine};

pub fn run(home: &ShiroHome, path: &str) -> Result<CmdOutput, ShiroError> {
    let engine = Engine::open(home.clone())?;
    let parser = shiro_parse::PlainTextParser;

    let input = AddInput {
        path: path.to_string(),
    };
    let output = engine.add(&parser, &input)?;

    let result = serde_json::json!({
        "doc_id": output.doc_id,
        "status": output.status,
        "title": output.title,
        "segments": output.segments,
        "changed": output.changed,
    });

    Ok(CmdOutput {
        result,
        next_actions: read_next_actions(&output.doc_id),
    })
}

fn read_next_actions(doc_id: &str) -> Vec<NextAction> {
    let mut params = BTreeMap::new();
    params.insert(
        "doc_id".to_string(),
        ParamMeta {
            value: Some(serde_json::json!(doc_id)),
            default: None,
            description: Some("Document ID".to_string()),
        },
    );

    vec![
        NextAction::with_params(
            "shiro read <doc_id> --text",
            "Read document text",
            params.clone(),
        ),
        NextAction::with_params(
            "shiro read <doc_id> --blocks",
            "View document blocks",
            params,
        ),
        NextAction::simple("shiro search <query>", "Search the library"),
    ]
}
