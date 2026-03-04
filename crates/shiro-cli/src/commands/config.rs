//! `shiro config` — show/get/set configuration.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};

pub fn run_show(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let result = serde_json::json!({
        "home": home.root().as_str(),
        "db_path": home.db_path().as_str(),
        "tantivy_dir": home.tantivy_dir().as_str(),
        "config_path": home.config_path().as_str(),
        "lock_dir": home.lock_dir().as_str(),
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![NextAction::simple("shiro doctor", "Check library health")],
    })
}

pub fn run_get(_home: &ShiroHome, key: &str) -> Result<CmdOutput, ShiroError> {
    // TODO: read from config.toml when config persistence is implemented.
    // Acceptance: read values from config.toml; return typed values.
    Err(ShiroError::Config {
        message: format!("config key '{key}' not found (config persistence not yet implemented)"),
    })
}

pub fn run_set(_home: &ShiroHome, key: &str, _value: &str) -> Result<CmdOutput, ShiroError> {
    // TODO: write to config.toml when config persistence is implemented.
    // Acceptance: persist to config.toml; validate key/value types.
    Err(ShiroError::Config {
        message: format!("config set for '{key}' not yet implemented (config persistence pending)"),
    })
}
