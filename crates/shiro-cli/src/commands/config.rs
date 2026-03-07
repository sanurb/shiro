//! `shiro config` — show/get/set configuration.
//!
//! Operates on a typed [`ShiroConfig`] model. Unknown keys are rejected,
//! values are parsed according to each field's declared type, and writes
//! are atomic (temp file + rename).

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::config::{
    lookup_key, ConfigFieldKind, EmbedConfig, SearchConfig, ShiroConfig, CONFIG_KEYS,
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
    let value = get_field(&config, key).ok_or_else(|| ShiroError::Config {
        message: format!("config key '{key}' is not set"),
    })?;
    Ok(CmdOutput {
        result: serde_json::json!({ "key": key, "value": value }),
        next_actions: vec![NextAction::simple(
            "shiro config show",
            "Show all configuration",
        )],
    })
}

pub fn run_set(home: &ShiroHome, key: &str, value: &str) -> Result<CmdOutput, ShiroError> {
    let meta = validate_key(key)?;
    let config_path = home.config_path();

    let mut config = load_config(home)?;
    set_field(&mut config, key, value, meta.kind)?;
    write_config_atomic(&config_path, &config)?;

    // Return the value we just stored — no re-read needed because we
    // operate on the typed model, not a raw document.
    let stored = get_field(&config, key);
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

/// Read a field from the typed config by dotted key.
fn get_field(config: &ShiroConfig, key: &str) -> Option<serde_json::Value> {
    match key {
        "search.limit" => config
            .search
            .as_ref()
            .and_then(|s| s.limit)
            .map(|v| serde_json::json!(v)),
        "embed.base_url" => config
            .embed
            .as_ref()
            .and_then(|e| e.base_url.as_deref())
            .map(|v| serde_json::json!(v)),
        "embed.model" => config
            .embed
            .as_ref()
            .and_then(|e| e.model.as_deref())
            .map(|v| serde_json::json!(v)),
        "embed.dimensions" => config
            .embed
            .as_ref()
            .and_then(|e| e.dimensions)
            .map(|v| serde_json::json!(v)),
        "embed.api_key" => config
            .embed
            .as_ref()
            .and_then(|e| e.api_key.as_deref())
            .map(|v| serde_json::json!(v)),
        _ => None,
    }
}

/// Set a field on the typed config, parsing `raw` according to the field's
/// declared type. Returns a precise error on parse failure.
fn set_field(
    config: &mut ShiroConfig,
    key: &str,
    raw: &str,
    kind: ConfigFieldKind,
) -> Result<(), ShiroError> {
    match key {
        "search.limit" => {
            let v = parse_value::<u32>(raw, kind)?;
            config
                .search
                .get_or_insert_with(SearchConfig::default)
                .limit = Some(v);
        }
        "embed.base_url" => {
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .base_url = Some(raw.to_string());
        }
        "embed.model" => {
            config.embed.get_or_insert_with(EmbedConfig::default).model = Some(raw.to_string());
        }
        "embed.dimensions" => {
            let v = parse_value::<usize>(raw, kind)?;
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .dimensions = Some(v);
        }
        "embed.api_key" => {
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .api_key = Some(raw.to_string());
        }
        _ => {
            // Should never reach here — validate_key guards the entry.
            return Err(ShiroError::InvalidInput {
                message: format!("unhandled config key '{key}'"),
            });
        }
    }
    Ok(())
}

/// Parse a raw CLI string into type `T` according to the declared field kind.
/// Produces a human-readable error on failure.
fn parse_value<T: std::str::FromStr>(raw: &str, kind: ConfigFieldKind) -> Result<T, ShiroError>
where
    T::Err: std::fmt::Display,
{
    raw.parse::<T>().map_err(|e| ShiroError::InvalidInput {
        message: format!(
            "invalid value '{raw}' for {kind_name} field: {e}",
            kind_name = kind_name(kind)
        ),
    })
}

fn kind_name(kind: ConfigFieldKind) -> &'static str {
    match kind {
        ConfigFieldKind::U32 => "u32",
        ConfigFieldKind::Usize => "usize",
        ConfigFieldKind::Str => "string",
        ConfigFieldKind::Bool => "bool",
    }
}

// ---------------------------------------------------------------------------
// File I/O — typed, safe
// ---------------------------------------------------------------------------

/// Load config from disk. Returns `ShiroConfig::default()` if the file
/// does not exist (first-run). Any other I/O or parse error propagates.
fn load_config(home: &ShiroHome) -> Result<ShiroConfig, ShiroError> {
    let config_path = home.config_path();
    match std::fs::read_to_string(config_path.as_std_path()) {
        Ok(content) => toml::from_str::<ShiroConfig>(&content).map_err(|e| ShiroError::Config {
            message: format!("parse {}: {e}", config_path),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ShiroConfig::default()),
        Err(e) => Err(ShiroError::Config {
            message: format!("read {}: {e}", config_path),
        }),
    }
}

/// Atomic write: serialize → write to temp file in same dir → rename.
///
/// Rename on the same filesystem is atomic on POSIX. On Windows it is
/// close-to-atomic (ReplaceFile). Either way, the config file is never
/// left in a partial state.
fn write_config_atomic(
    config_path: &camino::Utf8Path,
    config: &ShiroConfig,
) -> Result<(), ShiroError> {
    let serialized = toml::to_string_pretty(config).map_err(|e| ShiroError::Config {
        message: format!("serialize config: {e}"),
    })?;

    // Ensure parent directory exists.
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent.as_std_path()).map_err(|e| ShiroError::Config {
            message: format!("create config dir {}: {e}", parent),
        })?;
    }

    // Write to a temp file in the same directory, then rename.
    let tmp_path = config_path.with_extension("toml.tmp");
    std::fs::write(tmp_path.as_std_path(), serialized.as_bytes()).map_err(|e| {
        ShiroError::Config {
            message: format!("write temp config {}: {e}", tmp_path),
        }
    })?;

    std::fs::rename(tmp_path.as_std_path(), config_path.as_std_path()).map_err(|e| {
        // Clean up temp file on rename failure.
        let _ = std::fs::remove_file(tmp_path.as_std_path());
        ShiroError::Config {
            message: format!("atomic rename {} → {}: {e}", tmp_path, config_path),
        }
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
            search: Some(SearchConfig { limit: Some(42) }),
            ..Default::default()
        };
        write_config_atomic(&home.config_path(), &cfg).unwrap();
        let loaded = load_config(&home).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn atomic_write_does_not_leave_tmp() {
        let (_dir, home) = test_home();
        let cfg = ShiroConfig::default();
        write_config_atomic(&home.config_path(), &cfg).unwrap();
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
        set_field(&mut cfg, "search.limit", "20", ConfigFieldKind::U32).unwrap();
        let val = get_field(&cfg, "search.limit");
        assert_eq!(val, Some(serde_json::json!(20)));
    }

    #[test]
    fn set_u32_rejects_non_numeric() {
        let mut cfg = ShiroConfig::default();
        let err = set_field(&mut cfg, "search.limit", "abc", ConfigFieldKind::U32).unwrap_err();
        assert!(err.to_string().contains("invalid value"), "got: {err}");
    }

    #[test]
    fn set_u32_rejects_negative() {
        let mut cfg = ShiroConfig::default();
        let err = set_field(&mut cfg, "search.limit", "-5", ConfigFieldKind::U32).unwrap_err();
        assert!(err.to_string().contains("invalid value"), "got: {err}");
    }

    #[test]
    fn set_and_get_string() {
        let mut cfg = ShiroConfig::default();
        set_field(
            &mut cfg,
            "embed.base_url",
            "http://localhost:11434/v1",
            ConfigFieldKind::Str,
        )
        .unwrap();
        let val = get_field(&cfg, "embed.base_url");
        assert_eq!(val, Some(serde_json::json!("http://localhost:11434/v1")));
    }

    #[test]
    fn get_unset_field_returns_none() {
        let cfg = ShiroConfig::default();
        assert_eq!(get_field(&cfg, "search.limit"), None);
    }

    #[test]
    fn load_rejects_unknown_toml_keys() {
        let (_dir, home) = test_home();
        let bad_toml = "[search]\nlimit = 10\n\n[bogus]\nfoo = true\n";
        std::fs::create_dir_all(home.config_path().parent().unwrap().as_std_path()).unwrap();
        std::fs::write(home.config_path().as_std_path(), bad_toml).unwrap();
        let err = load_config(&home).unwrap_err();
        assert!(
            err.to_string().contains("parse"),
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
}
