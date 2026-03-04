//! Self-documenting root command (no subcommand).

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::ShiroError;

pub fn run() -> Result<CmdOutput, ShiroError> {
    let result = serde_json::json!({
        "description": "shiro \u{2014} local-first PDF/Markdown knowledge engine",
        "commands": [
            { "name": "init", "usage": "shiro init" },
            { "name": "add", "usage": "shiro add <path|url> [--enrich] [--tags <csv>] [--concepts <csv>] [--parser <baseline|premium>] [--fts-only] [--follow]" },
            { "name": "ingest", "usage": "shiro ingest <dir...> [--glob <pattern>] [--enrich] [--tags <csv>] [--concepts <csv>] [--parser <baseline|premium>] [--max-files <n>] [--fts-only] [--follow]" },
            { "name": "search", "usage": "shiro search <query> [--vector|--bm25|--hybrid] [--limit <n>] [--expand] [--tag <tag>] [--concept <id>] [--doc <doc_id>]" },
            { "name": "read", "usage": "shiro read <doc_id|title> [--outline|--text|--blocks]" },
            { "name": "explain", "usage": "shiro explain <result_id>" },
            { "name": "list", "usage": "shiro list [--tag <tag>] [--concept <id>] [--limit <n>]" },
            { "name": "remove", "usage": "shiro remove <doc_id|title> [--purge]" },
            { "name": "config", "usage": "shiro config <show|get|set> ..." },
            { "name": "doctor", "usage": "shiro doctor [--verify-vector] [--repair]" },
            { "name": "mcp", "usage": "shiro mcp" },
        ]
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro init", "Initialize a new library"),
            NextAction::simple("shiro doctor", "Check library health"),
        ],
    })
}
