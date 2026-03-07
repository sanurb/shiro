//! Self-documenting root command (no subcommand).

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::ShiroError;
use std::collections::BTreeMap;

pub fn run() -> Result<CmdOutput, ShiroError> {
    let result = serde_json::json!({
        "description": "shiro \u{2014} local-first PDF/Markdown knowledge engine",
        "commands": [
            { "name": "init", "usage": "shiro init" },
            { "name": "add", "usage": "shiro add <path|url>" },
            { "name": "ingest", "usage": "shiro ingest <dir...> [--max-files <n>] [--follow]" },
            { "name": "search", "usage": "shiro search <query> [--vector|--bm25|--hybrid] [--limit <n>] [--expand] [--tag <tag>] [--concept <id>] [--doc <doc_id>]" },
            { "name": "read", "usage": "shiro read <doc_id|title> [--outline|--text|--blocks]" },
            { "name": "explain", "usage": "shiro explain <result_id>" },
            { "name": "list", "usage": "shiro list [--tag <tag>] [--concept <id>] [--limit <n>]" },
            { "name": "remove", "usage": "shiro remove <doc_id|title> [--purge]" },
            { "name": "taxonomy", "usage": "shiro taxonomy <subcommand> ..." },
            { "name": "config", "usage": "shiro config <show|get|set> ..." },
            { "name": "doctor", "usage": "shiro doctor [--verify-vector]" },
            { "name": "reindex", "usage": "shiro reindex [--fts] [--vector] [--follow]" },
            { "name": "mcp", "usage": "shiro mcp" },
            { "name": "completions", "usage": "shiro completions <shell>" },
            { "name": "enrich", "usage": "shiro enrich <doc_id>" },
            { "name": "capabilities", "usage": "shiro capabilities" }
        ]
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro doctor", "Check library health"),
            NextAction::with_params(
                "shiro list [--limit <n>]",
                "List documents",
                BTreeMap::from([(
                    "n".into(),
                    ParamMeta {
                        value: None,
                        default: Some(serde_json::json!(20)),
                        description: Some("Max documents".into()),
                    },
                )]),
            ),
        ],
    })
}
