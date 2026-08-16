//! `shiro config` — show/get/set configuration.
//!
//! Operates on a typed [`ShiroConfig`] model. Unknown keys are rejected,
//! values are parsed according to each field's declared type, and writes
//! are atomic (temp file + rename).

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::config::{
    get_config_value, is_sensitive_key, load_config, lookup_key, set_config_value,
    write_config_atomic, CONFIG_KEYS,
};
use shiro_core::{ShiroError, ShiroHome};

// ---------------------------------------------------------------------------
// Sub-commands
// ---------------------------------------------------------------------------

pub fn run_show(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let config = load_config(home)?;
    let values = serde_json::to_value(&config).map_err(|e| ShiroError::Config {
        message: format!("serialize config: {e}"),
    })?;
    let values = redact_config_values(values);
    let result = serde_json::json!({
        "home": home.root().as_str(),
        "db_path": home.db_path().as_str(),
        "tantivy_dir": home.tantivy_dir().as_str(),
        "config_path": home.config_path().as_str(),
        "lock_dir": home.lock_dir().as_str(),
        "values": values,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![NextAction::simple("shiro doctor", "Check library health")],
    })
}

pub fn run_get(home: &ShiroHome, key: &str) -> Result<CmdOutput, ShiroError> {
    validate_key(key)?;
    let config = load_config(home)?;
    let value = get_config_value(&config, key)
        .map(config_value_to_json)
        .transpose()?
        .ok_or_else(|| ShiroError::Config {
            message: format!("config key '{key}' is not set"),
        })?;
    let value = redact_value_for_key(key, value);
    Ok(CmdOutput {
        result: serde_json::json!({ "key": key, "value": value }),
        next_actions: vec![NextAction::simple(
            "shiro config show",
            "Show all configuration",
        )],
    })
}

pub fn run_set(home: &ShiroHome, key: &str, value: &str) -> Result<CmdOutput, ShiroError> {
    validate_key(key)?;

    let mut config = load_config(home)?;
    set_config_value(&mut config, key, value)?;
    write_config_atomic(home, &config)?;

    // Return the value we just stored — no re-read needed because we
    // operate on the typed model, not a raw document.
    let stored = get_config_value(&config, key)
        .map(config_value_to_json)
        .transpose()?
        .map(|value| redact_value_for_key(key, value));
    Ok(CmdOutput {
        result: serde_json::json!({ "key": key, "value": stored }),
        next_actions: vec![NextAction::simple(
            "shiro config show",
            "Show all configuration",
        )],
    })
}

// ---------------------------------------------------------------------------
// Key validation
// ---------------------------------------------------------------------------

/// Validate that `key` is a known config key. Returns its metadata.
fn validate_key(key: &str) -> Result<&'static shiro_core::config::ConfigKeyMeta, ShiroError> {
    lookup_key(key).ok_or_else(|| {
        let valid: Vec<&str> = CONFIG_KEYS.iter().map(|m| m.key).collect();
        ShiroError::InvalidInput {
            message: format!(
                "unknown config key '{key}'; valid keys: {}",
                valid.join(", ")
            ),
        }
    })
}

// ---------------------------------------------------------------------------
// Typed field access
// ---------------------------------------------------------------------------

fn config_value_to_json(value: toml::Value) -> Result<serde_json::Value, ShiroError> {
    serde_json::to_value(value).map_err(|error| ShiroError::Config {
        message: format!("serialize config value: {error}"),
    })
}

fn redact_value_for_key(key: &str, value: serde_json::Value) -> serde_json::Value {
    if is_sensitive_key(key) {
        serde_json::json!("***REDACTED***")
    } else {
        value
    }
}

fn redact_config_values(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(api_key) = value.pointer_mut("/embed/api_key") {
        if !api_key.is_null() {
            *api_key = serde_json::json!("***REDACTED***");
        }
    }
    value
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use shiro_core::config::{SearchConfig, ShiroConfig};

    use super::*;

    fn test_home() -> (tempfile::TempDir, ShiroHome) {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap();
        let home = ShiroHome::new(root.to_path_buf());
        (dir, home)
    }

    #[test]
    fn load_missing_file_returns_default() {
        let (_dir, home) = test_home();
        let cfg = load_config(&home).unwrap();
        assert_eq!(cfg, ShiroConfig::default());
    }

    #[test]
    fn roundtrip_write_read() {
        let (_dir, home) = test_home();
        let cfg = ShiroConfig {
            version: shiro_core::config::CURRENT_CONFIG_VERSION,
            search: Some(SearchConfig { limit: Some(42) }),
            ..Default::default()
        };
        write_config_atomic(&home, &cfg).unwrap();
        let loaded = load_config(&home).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn atomic_write_does_not_leave_tmp() {
        let (_dir, home) = test_home();
        let cfg = ShiroConfig::default();
        write_config_atomic(&home, &cfg).unwrap();
        let tmp = home.config_path().with_extension("toml.tmp");
        assert!(
            !tmp.as_std_path().exists(),
            "temp file should be removed after rename"
        );
    }

    #[test]
    fn validate_key_rejects_unknown() {
        let err = validate_key("bogus.key").unwrap_err();
        assert!(err.to_string().contains("unknown config key"), "got: {err}");
    }

    #[test]
    fn validate_key_accepts_known() {
        assert!(validate_key("search.limit").is_ok());
        assert!(validate_key("embed.base_url").is_ok());
    }

    #[test]
    fn set_and_get_u32() {
        let mut cfg = ShiroConfig::default();
        set_config_value(&mut cfg, "search.limit", "20").unwrap();
        let val = get_config_value(&cfg, "search.limit")
            .map(config_value_to_json)
            .transpose()
            .unwrap();
        assert_eq!(val, Some(serde_json::json!(20)));
    }

    #[test]
    fn set_u32_rejects_non_numeric() {
        let mut cfg = ShiroConfig::default();
        let err = set_config_value(&mut cfg, "search.limit", "abc").unwrap_err();
        assert!(err.to_string().contains("invalid value"), "got: {err}");
    }

    #[test]
    fn set_u32_rejects_negative() {
        let mut cfg = ShiroConfig::default();
        let err = set_config_value(&mut cfg, "search.limit", "-5").unwrap_err();
        assert!(err.to_string().contains("invalid value"), "got: {err}");
    }

    #[test]
    fn set_and_get_string() {
        let mut cfg = ShiroConfig::default();
        set_config_value(&mut cfg, "embed.base_url", "http://localhost:11434/v1").unwrap();
        let val = get_config_value(&cfg, "embed.base_url")
            .map(config_value_to_json)
            .transpose()
            .unwrap();
        assert_eq!(val, Some(serde_json::json!("http://localhost:11434/v1")));
    }

    #[test]
    fn get_unset_field_returns_none() {
        let cfg = ShiroConfig::default();
        assert_eq!(get_config_value(&cfg, "search.limit"), None);
    }

    #[test]
    fn load_rejects_unknown_toml_keys() {
        let (_dir, home) = test_home();
        let bad_toml = "[search]\nlimit = 10\n\n[bogus]\nfoo = true\n";
        std::fs::create_dir_all(home.config_path().parent().unwrap().as_std_path()).unwrap();
        std::fs::write(home.config_path().as_std_path(), bad_toml).unwrap();
        let err = load_config(&home).unwrap_err();
        assert!(
            err.to_string().contains("unknown field `bogus`"),
            "should reject unknown section, got: {err}"
        );
    }

    #[test]
    fn run_show_includes_paths() {
        let (_dir, home) = test_home();
        let output = run_show(&home).unwrap();
        assert!(output.result["home"].is_string());
        assert!(output.result["config_path"].is_string());
        assert!(output.result["values"].is_object());
    }

    #[test]
    fn run_get_unknown_key_is_invalid_input() {
        let (_dir, home) = test_home();
        let err = run_get(&home, "nonexistent").unwrap_err();
        let code = shiro_core::ErrorCode::from_error(&err);
        assert_eq!(
            code,
            shiro_core::ErrorCode::EInvalidInput,
            "unknown key should be InvalidInput, got: {code}"
        );
    }

    #[test]
    fn run_set_then_get() {
        let (_dir, home) = test_home();
        let out = run_set(&home, "search.limit", "50").unwrap();
        assert_eq!(out.result["key"].as_str().unwrap(), "search.limit");
        assert_eq!(out.result["value"].as_u64().unwrap(), 50);

        let out = run_get(&home, "search.limit").unwrap();
        assert_eq!(out.result["value"].as_u64().unwrap(), 50);
    }

    #[test]
    fn run_set_bad_value_type() {
        let (_dir, home) = test_home();
        let err = run_set(&home, "search.limit", "not_a_number").unwrap_err();
        assert!(err.to_string().contains("invalid value"), "got: {err}");
    }

    #[test]
    fn api_key_is_redacted_in_command_output() {
        let (_dir, home) = test_home();
        let set = run_set(&home, "embed.api_key", "secret").unwrap();
        assert_eq!(set.result["value"].as_str().unwrap(), "***REDACTED***");

        let get = run_get(&home, "embed.api_key").unwrap();
        assert_eq!(get.result["value"].as_str().unwrap(), "***REDACTED***");

        let show = run_show(&home).unwrap();
        assert_eq!(
            show.result["values"]["embed"]["api_key"].as_str().unwrap(),
            "***REDACTED***"
        );
    }
}
