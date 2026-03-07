//! Configuration types and `ShiroHome` path management.
//!
//! `ShiroHome` resolves all derived paths (DB, indices, config) from a
//! single root directory. Default: `~/.shiro`.
//!
//! [`ShiroConfig`] is the typed configuration model. It is the single
//! authoritative schema for `config.toml` — unknown keys are rejected
//! at parse time via `#[serde(deny_unknown_fields)]`.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

/// Manages the shiro home directory layout.
///
/// ```text
/// <root>/
///   config.toml
///   shiro.db              (SQLite — source of truth)
///   tantivy/              (FTS index)
///   lock/write.lock       (single-writer lock)
/// ```
#[derive(Debug, Clone)]
pub struct ShiroHome {
    root: Utf8PathBuf,
}

impl ShiroHome {
    pub fn new(root: Utf8PathBuf) -> Self {
        Self { root }
    }

    /// Resolve the default home directory (`~/.shiro`).
    ///
    /// Precedence: `explicit` arg > `SHIRO_HOME` env > `~/.shiro`.
    pub fn resolve(explicit: Option<&str>) -> Result<Self, String> {
        if let Some(p) = explicit {
            return Ok(Self::new(Utf8PathBuf::from(p)));
        }

        if let Ok(env) = std::env::var("SHIRO_HOME") {
            return Ok(Self::new(Utf8PathBuf::from(env)));
        }

        let home = dirs_path()
            .map(|h| h.join(".shiro"))
            .ok_or_else(|| "cannot determine home directory".to_string())?;
        Ok(Self::new(home))
    }

    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    pub fn db_path(&self) -> Utf8PathBuf {
        self.root.join("shiro.db")
    }

    pub fn tantivy_dir(&self) -> Utf8PathBuf {
        self.root.join("tantivy")
    }

    /// Path to the staging tantivy directory for generational rebuilds.
    pub fn staging_tantivy_dir(&self) -> Utf8PathBuf {
        self.root.join("tantivy_staging")
    }

    pub fn config_path(&self) -> Utf8PathBuf {
        self.root.join("config.toml")
    }

    pub fn lock_dir(&self) -> Utf8PathBuf {
        self.root.join("lock")
    }

    pub fn vector_dir(&self) -> Utf8PathBuf {
        self.root.join("vector")
    }

    pub fn staging_vector_dir(&self) -> Utf8PathBuf {
        self.root.join("vector_staging")
    }

    /// Create the directory structure if it does not exist.
    pub fn ensure_dirs(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(self.root.as_std_path())?;
        std::fs::create_dir_all(self.tantivy_dir().as_std_path())?;
        std::fs::create_dir_all(self.vector_dir().as_std_path())?;
        std::fs::create_dir_all(self.lock_dir().as_std_path())?;
        Ok(())
    }
}

/// Platform-aware home directory.
fn dirs_path() -> Option<Utf8PathBuf> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok().map(Utf8PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(Utf8PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

// ---------------------------------------------------------------------------
// Typed configuration model
// ---------------------------------------------------------------------------

/// Typed representation of `config.toml`.
///
/// This is the single authoritative config schema. All reads and writes
/// go through this type — there is no untyped fallback. Unknown keys
/// are rejected at deserialization time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ShiroConfig {
    /// Search-related settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchConfig>,

    /// Embedding service settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<EmbedConfig>,
}

/// Search configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SearchConfig {
    /// Maximum number of results returned by search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Embedding service configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EmbedConfig {
    /// Base URL of the embedding service (e.g., `http://localhost:11434/v1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Model name to request (e.g., `all-minilm`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Expected embedding dimensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<usize>,

    /// Optional API key for authenticated endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// The type a config field expects, used for schema-aware parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFieldKind {
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned pointer-sized integer.
    Usize,
    /// UTF-8 string.
    Str,
    /// Boolean.
    Bool,
}

/// Descriptor for a single known config key.
#[derive(Debug, Clone, Copy)]
pub struct ConfigKeyMeta {
    /// Dotted key path (e.g., `search.limit`).
    pub key: &'static str,
    /// Expected value type.
    pub kind: ConfigFieldKind,
    /// Human description.
    pub description: &'static str,
}

/// Registry of all known config keys.
pub const CONFIG_KEYS: &[ConfigKeyMeta] = &[
    ConfigKeyMeta {
        key: "search.limit",
        kind: ConfigFieldKind::U32,
        description: "Maximum search results",
    },
    ConfigKeyMeta {
        key: "embed.base_url",
        kind: ConfigFieldKind::Str,
        description: "Embedding service base URL",
    },
    ConfigKeyMeta {
        key: "embed.model",
        kind: ConfigFieldKind::Str,
        description: "Embedding model name",
    },
    ConfigKeyMeta {
        key: "embed.dimensions",
        kind: ConfigFieldKind::Usize,
        description: "Expected embedding dimensions",
    },
    ConfigKeyMeta {
        key: "embed.api_key",
        kind: ConfigFieldKind::Str,
        description: "Embedding service API key",
    },
];

/// Look up a config key's metadata. Returns `None` for unknown keys.
pub fn lookup_key(key: &str) -> Option<&'static ConfigKeyMeta> {
    CONFIG_KEYS.iter().find(|m| m.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_paths() {
        let home = ShiroHome::new(Utf8PathBuf::from("/tmp/test-shiro"));
        assert_eq!(home.db_path().as_str(), "/tmp/test-shiro/shiro.db");
        assert_eq!(home.tantivy_dir().as_str(), "/tmp/test-shiro/tantivy");
        assert_eq!(home.config_path().as_str(), "/tmp/test-shiro/config.toml");
    }

    #[test]
    fn explicit_override() {
        let home = ShiroHome::resolve(Some("/custom/path")).unwrap();
        assert_eq!(home.root().as_str(), "/custom/path");
    }

    #[test]
    fn config_default_is_empty() {
        let cfg = ShiroConfig::default();
        assert_eq!(cfg.search, None);
        assert_eq!(cfg.embed, None);
    }

    #[test]
    fn config_roundtrip_toml() {
        // Verify serde Serialize+Deserialize work (toml tested in CLI).
        let cfg = ShiroConfig {
            search: Some(SearchConfig { limit: Some(25) }),
            embed: Some(EmbedConfig {
                base_url: Some("http://localhost:11434/v1".into()),
                model: Some("all-minilm".into()),
                dimensions: Some(384),
                api_key: None,
            }),
        };
        // Round-trip via serde_json (dev-dep).
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ShiroConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn config_rejects_unknown_keys() {
        let bad = r#"{"search": {"limit": 10}, "bogus": true}"#;
        let err = serde_json::from_str::<ShiroConfig>(bad);
        assert!(err.is_err(), "deny_unknown_fields should reject 'bogus'");
    }

    #[test]
    fn lookup_key_found() {
        let meta = lookup_key("search.limit");
        assert!(meta.is_some());
        assert_eq!(meta.unwrap().kind, ConfigFieldKind::U32);
    }

    #[test]
    fn lookup_key_unknown() {
        assert!(lookup_key("bogus.key").is_none());
    }
}
