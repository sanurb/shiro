//! `shiro init` — create storage layout and SQLite schema.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    home.ensure_dirs().map_err(|e| ShiroError::Config {
        message: format!("failed to create directories: {e}"),
    })?;

    // Initialize SQLite schema.
    let _store = Store::open(&home.db_path())?;
    tracing::info!(path = %home.db_path(), "initialized SQLite store");

    // Initialize Tantivy index.
    let _fts = FtsIndex::open(&home.tantivy_dir())?;
    tracing::info!(path = %home.tantivy_dir(), "initialized FTS index");

    let result = serde_json::json!({
        "home": home.root().as_str(),
        "created": true,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro doctor", "Check library health"),
            NextAction::simple("shiro add <path|url>", "Add a document to the library"),
        ],
    })
}
