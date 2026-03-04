//! `shiro doctor` — consistency checks and diagnostics.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let mut checks = Vec::new();

    // Check 1: home directory exists.
    let home_exists = home.root().as_std_path().is_dir();
    checks.push(serde_json::json!({
        "name": "home_directory",
        "status": if home_exists { "ok" } else { "fail" },
        "message": if home_exists {
            format!("{} exists", home.root())
        } else {
            format!("{} not found — run `shiro init`", home.root())
        },
    }));

    if !home_exists {
        let result = serde_json::json!({ "checks": checks, "healthy": false });
        return Ok(CmdOutput {
            result,
            next_actions: vec![NextAction::simple("shiro init", "Initialize the library")],
        });
    }

    // Check 2: SQLite database.
    let db_check = match Store::open(&home.db_path()) {
        Ok(store) => {
            let counts = store.count_by_state().unwrap_or_default();
            let total: usize = counts.iter().map(|(_, c)| c).sum();
            serde_json::json!({
                "name": "sqlite_store",
                "status": "ok",
                "message": format!("{total} documents in store"),
                "details": counts.iter().map(|(s, c)| {
                    serde_json::json!({ "state": s.as_str(), "count": c })
                }).collect::<Vec<_>>(),
            })
        }
        Err(e) => {
            serde_json::json!({
                "name": "sqlite_store",
                "status": "fail",
                "message": format!("cannot open store: {e}"),
            })
        }
    };
    let db_ok = db_check["status"].as_str() == Some("ok");
    checks.push(db_check);

    // Check 3: Tantivy FTS index.
    let fts_check = match FtsIndex::open(&home.tantivy_dir()) {
        Ok(fts) => {
            let count = fts.num_segments().unwrap_or(0);
            serde_json::json!({
                "name": "fts_index",
                "status": "ok",
                "message": format!("{count} segments indexed"),
            })
        }
        Err(e) => {
            serde_json::json!({
                "name": "fts_index",
                "status": "fail",
                "message": format!("cannot open FTS index: {e}"),
            })
        }
    };
    let fts_ok = fts_check["status"].as_str() == Some("ok");
    checks.push(fts_check);

    let healthy = home_exists && db_ok && fts_ok;
    let result = serde_json::json!({ "checks": checks, "healthy": healthy });

    let mut next_actions = Vec::new();
    if !healthy {
        next_actions.push(NextAction::simple(
            "shiro init",
            "Re-initialize the library",
        ));
    }
    next_actions.push(NextAction::simple("shiro list", "List documents"));

    Ok(CmdOutput {
        result,
        next_actions,
    })
}
