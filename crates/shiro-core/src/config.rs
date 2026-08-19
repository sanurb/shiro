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

use crate::ShiroError;

/// Current on-disk configuration schema version.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ShiroConfig {
    /// Configuration schema version. Written on every config write.
    #[serde(default = "current_config_version")]
    pub version: u32,

    /// Search-related settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchConfig>,

    /// Ingestion enrichment settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingest: Option<IngestConfig>,

    /// Embedding service settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<EmbedConfig>,

    /// Reranking settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank: Option<RerankConfig>,
}

fn current_config_version() -> u32 {
    CURRENT_CONFIG_VERSION
}

impl Default for ShiroConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_CONFIG_VERSION,
            search: None,
            ingest: None,
            embed: None,
            rerank: None,
        }
    }
}

/// Search configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SearchConfig {
    /// Maximum number of results returned by search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Ingestion enrichment configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct IngestConfig {
    /// Propose assignments to existing taxonomy concepts after ingest.
    pub auto_concept_proposals: bool,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            auto_concept_proposals: true,
        }
    }
}

/// Embedding service configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EmbedConfig {
    /// Embedding provider (`"http"`, `"fastembed"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

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

    /// FastEmbed model cache directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<String>,
}

/// Reranking configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RerankConfig {
    /// Reranking provider (`"fastembed"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Reranking model name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Number of results to rerank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
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
    /// Whether values for this key must be redacted in output.
    pub sensitive: bool,
    /// Human description.
    pub description: &'static str,
}

/// Registry of all known config keys.
pub const CONFIG_KEYS: &[ConfigKeyMeta] = &[
    ConfigKeyMeta {
        key: "search.limit",
        kind: ConfigFieldKind::U32,
        sensitive: false,
        description: "Maximum search results",
    },
    ConfigKeyMeta {
        key: "ingest.auto_concept_proposals",
        kind: ConfigFieldKind::Bool,
        sensitive: false,
        description: "Propose existing taxonomy concepts during ingest (default true)",
    },
    ConfigKeyMeta {
        key: "embed.base_url",
        kind: ConfigFieldKind::Str,
        sensitive: false,
        description: "Embedding service base URL",
    },
    ConfigKeyMeta {
        key: "embed.model",
        kind: ConfigFieldKind::Str,
        sensitive: false,
        description: "Embedding model name",
    },
    ConfigKeyMeta {
        key: "embed.dimensions",
        kind: ConfigFieldKind::Usize,
        sensitive: false,
        description: "Expected embedding dimensions",
    },
    ConfigKeyMeta {
        key: "embed.api_key",
        kind: ConfigFieldKind::Str,
        sensitive: true,
        description: "Embedding service API key",
    },
    ConfigKeyMeta {
        key: "embed.provider",
        kind: ConfigFieldKind::Str,
        sensitive: false,
        description: "Embedding provider (http, fastembed)",
    },
    ConfigKeyMeta {
        key: "embed.cache_dir",
        kind: ConfigFieldKind::Str,
        sensitive: false,
        description: "FastEmbed model cache directory",
    },
    ConfigKeyMeta {
        key: "rerank.provider",
        kind: ConfigFieldKind::Str,
        sensitive: false,
        description: "Reranking provider (fastembed)",
    },
    ConfigKeyMeta {
        key: "rerank.model",
        kind: ConfigFieldKind::Str,
        sensitive: false,
        description: "Reranking model name",
    },
    ConfigKeyMeta {
        key: "rerank.top_k",
        kind: ConfigFieldKind::Usize,
        sensitive: false,
        description: "Number of results to rerank",
    },
];

/// Look up a config key's metadata. Returns `None` for unknown keys.
pub fn lookup_key(key: &str) -> Option<&'static ConfigKeyMeta> {
    CONFIG_KEYS.iter().find(|m| m.key == key)
}

/// Return whether a config key must be redacted from output and errors.
pub fn is_sensitive_key(key: &str) -> bool {
    lookup_key(key).is_some_and(|meta| meta.sensitive)
}

/// Read one typed config field by its canonical dotted key.
pub fn get_config_value(config: &ShiroConfig, key: &str) -> Option<toml::Value> {
    match key {
        "search.limit" => config
            .search
            .as_ref()
            .and_then(|search| search.limit)
            .map(|value| toml::Value::Integer(i64::from(value))),
        "ingest.auto_concept_proposals" => Some(toml::Value::Boolean(
            config
                .ingest
                .as_ref()
                .map(|ingest| ingest.auto_concept_proposals)
                .unwrap_or(true),
        )),
        "embed.base_url" => config
            .embed
            .as_ref()
            .and_then(|embed| embed.base_url.clone())
            .map(toml::Value::String),
        "embed.model" => config
            .embed
            .as_ref()
            .and_then(|embed| embed.model.clone())
            .map(toml::Value::String),
        "embed.dimensions" => config
            .embed
            .as_ref()
            .and_then(|embed| embed.dimensions)
            .and_then(|value| i64::try_from(value).ok())
            .map(toml::Value::Integer),
        "embed.api_key" => config
            .embed
            .as_ref()
            .and_then(|embed| embed.api_key.clone())
            .map(toml::Value::String),
        "embed.provider" => config
            .embed
            .as_ref()
            .and_then(|embed| embed.provider.clone())
            .map(toml::Value::String),
        "embed.cache_dir" => config
            .embed
            .as_ref()
            .and_then(|embed| embed.cache_dir.clone())
            .map(toml::Value::String),
        "rerank.provider" => config
            .rerank
            .as_ref()
            .and_then(|rerank| rerank.provider.clone())
            .map(toml::Value::String),
        "rerank.model" => config
            .rerank
            .as_ref()
            .and_then(|rerank| rerank.model.clone())
            .map(toml::Value::String),
        "rerank.top_k" => config
            .rerank
            .as_ref()
            .and_then(|rerank| rerank.top_k)
            .and_then(|value| i64::try_from(value).ok())
            .map(toml::Value::Integer),
        _ => None,
    }
}

/// Parse and set one typed config field by its canonical dotted key.
pub fn set_config_value(config: &mut ShiroConfig, key: &str, raw: &str) -> Result<(), ShiroError> {
    let meta = lookup_key(key).ok_or_else(|| ShiroError::InvalidInput {
        message: format!("unknown config key '{key}'"),
    })?;
    match key {
        "search.limit" => {
            config
                .search
                .get_or_insert_with(SearchConfig::default)
                .limit = Some(parse_config_number(raw, meta.kind)?);
        }
        "ingest.auto_concept_proposals" => {
            config
                .ingest
                .get_or_insert_with(IngestConfig::default)
                .auto_concept_proposals =
                raw.parse::<bool>()
                    .map_err(|error| ShiroError::InvalidInput {
                        message: format!("invalid value '{raw}' for bool field: {error}"),
                    })?;
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
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .dimensions = Some(parse_config_number(raw, meta.kind)?);
        }
        "embed.api_key" => {
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .api_key = Some(raw.to_string());
        }
        "embed.provider" => {
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .provider = Some(raw.to_string());
        }
        "embed.cache_dir" => {
            config
                .embed
                .get_or_insert_with(EmbedConfig::default)
                .cache_dir = Some(raw.to_string());
        }
        "rerank.provider" => {
            config
                .rerank
                .get_or_insert_with(RerankConfig::default)
                .provider = Some(raw.to_string());
        }
        "rerank.model" => {
            config
                .rerank
                .get_or_insert_with(RerankConfig::default)
                .model = Some(raw.to_string());
        }
        "rerank.top_k" => {
            config
                .rerank
                .get_or_insert_with(RerankConfig::default)
                .top_k = Some(parse_config_number(raw, meta.kind)?);
        }
        _ => {
            return Err(ShiroError::InvalidInput {
                message: format!("unhandled config key '{key}'"),
            });
        }
    }
    Ok(())
}

fn parse_config_number<T>(raw: &str, kind: ConfigFieldKind) -> Result<T, ShiroError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    raw.parse::<T>().map_err(|error| ShiroError::InvalidInput {
        message: format!(
            "invalid value '{raw}' for {kind_name} field: {error}",
            kind_name = config_field_kind_name(kind)
        ),
    })
}

fn config_field_kind_name(kind: ConfigFieldKind) -> &'static str {
    match kind {
        ConfigFieldKind::U32 => "u32",
        ConfigFieldKind::Usize => "usize",
        ConfigFieldKind::Str => "string",
        ConfigFieldKind::Bool => "bool",
    }
}

/// Load, migrate, and validate `config.toml`.
///
/// Missing files return the code defaults. Existing files are parsed as TOML,
/// migrated sequentially to [`CURRENT_CONFIG_VERSION`], then deserialized
/// through the strict typed schema so unknown keys fail closed.
pub fn load_config(home: &ShiroHome) -> Result<ShiroConfig, ShiroError> {
    let config_path = home.config_path();
    match std::fs::read_to_string(config_path.as_std_path()) {
        Ok(content) => parse_config(&config_path, &content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ShiroConfig::default()),
        Err(error) => Err(ShiroError::Config {
            message: format!("read {config_path}: {error}"),
        }),
    }
}

/// Parse, migrate, and validate config content from an arbitrary source.
pub fn parse_config(path: &Utf8Path, content: &str) -> Result<ShiroConfig, ShiroError> {
    let value = content
        .parse::<toml::Value>()
        .map_err(|error| ShiroError::Config {
            message: format!("parse {path}: {error}"),
        })?;
    let migrated = migrate_config_value(path, value)?;
    migrated
        .try_into::<ShiroConfig>()
        .map_err(|error| ShiroError::Config {
            message: format!("validate {path}: {error}"),
        })
}

/// Atomically write the typed config to `config.toml`.
pub fn write_config_atomic(home: &ShiroHome, config: &ShiroConfig) -> Result<(), ShiroError> {
    let config_path = home.config_path();
    let mut config = config.clone();
    config.version = CURRENT_CONFIG_VERSION;
    let serialized = toml::to_string_pretty(&config).map_err(|error| ShiroError::Config {
        message: format!("serialize config: {error}"),
    })?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent.as_std_path()).map_err(|error| ShiroError::Config {
            message: format!("create config dir {parent}: {error}"),
        })?;
    }

    let tmp_path = config_path.with_extension("toml.tmp");
    std::fs::write(tmp_path.as_std_path(), serialized.as_bytes()).map_err(|error| {
        ShiroError::Config {
            message: format!("write temp config {tmp_path}: {error}"),
        }
    })?;

    std::fs::rename(tmp_path.as_std_path(), config_path.as_std_path()).map_err(|error| {
        let _ = std::fs::remove_file(tmp_path.as_std_path());
        ShiroError::Config {
            message: format!("atomic rename {tmp_path} -> {config_path}: {error}"),
        }
    })
}

fn migrate_config_value(
    path: &Utf8Path,
    mut value: toml::Value,
) -> Result<toml::Value, ShiroError> {
    let mut version = config_version(path, &value)?;
    if version > CURRENT_CONFIG_VERSION {
        return Err(ShiroError::Config {
            message: format!(
                "{path} uses config version {version}, but this shiro supports up to {CURRENT_CONFIG_VERSION}"
            ),
        });
    }

    while version < CURRENT_CONFIG_VERSION {
        value = match version {
            0 => migrate_v0_to_v1(value),
            other => {
                return Err(ShiroError::SchemaMigration {
                    message: format!("missing config migration {other} -> {}", other + 1),
                });
            }
        };
        version += 1;
    }

    Ok(value)
}

fn config_version(path: &Utf8Path, value: &toml::Value) -> Result<u32, ShiroError> {
    match value.get("version") {
        Some(version) => {
            let Some(version) = version.as_integer() else {
                return Err(ShiroError::Config {
                    message: format!("{path}: config version must be an integer"),
                });
            };
            u32::try_from(version).map_err(|_| ShiroError::Config {
                message: format!(
                    "{path}: config version must be between 0 and {u32_max}",
                    u32_max = u32::MAX
                ),
            })
        }
        None => Ok(0),
    }
}

fn migrate_v0_to_v1(mut value: toml::Value) -> toml::Value {
    if let Some(table) = value.as_table_mut() {
        table.insert(
            "version".to_string(),
            toml::Value::Integer(i64::from(CURRENT_CONFIG_VERSION)),
        );
    }
    value
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
        assert_eq!(cfg.version, CURRENT_CONFIG_VERSION);
        assert_eq!(cfg.search, None);
        assert_eq!(cfg.embed, None);
    }

    #[test]
    fn config_roundtrip_toml() {
        // Verify serde Serialize+Deserialize work (toml tested in CLI).
        let cfg = ShiroConfig {
            version: CURRENT_CONFIG_VERSION,
            search: Some(SearchConfig { limit: Some(25) }),
            ingest: Some(IngestConfig {
                auto_concept_proposals: false,
            }),
            embed: Some(EmbedConfig {
                provider: Some("http".into()),
                base_url: Some("http://localhost:11434/v1".into()),
                model: Some("all-minilm".into()),
                dimensions: Some(384),
                api_key: None,
                cache_dir: None,
            }),
            rerank: None,
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

    #[test]
    fn missing_version_migrates_to_current() {
        let path = Utf8Path::new("/tmp/config.toml");
        let cfg = parse_config(path, "[search]\nlimit = 10\n").unwrap();
        assert_eq!(cfg.version, CURRENT_CONFIG_VERSION);
        assert_eq!(cfg.search.unwrap().limit, Some(10));
    }

    #[test]
    fn future_version_is_rejected() {
        let path = Utf8Path::new("/tmp/config.toml");
        let err = parse_config(path, "version = 999\n").unwrap_err();
        assert!(err.to_string().contains("supports up to"));
    }

    #[test]
    fn rerank_candidate_limit_is_validated_when_parsing_config() {
        let path = Utf8Path::new("/tmp/config.toml");
        let zero = parse_config(path, "version = 1\n[rerank]\ntop_k = 0\n");
        let above_bound = parse_config(path, "version = 1\n[rerank]\ntop_k = 201\n");

        assert!(matches!(zero, Err(ShiroError::Config { .. })));
        assert!(matches!(above_bound, Err(ShiroError::Config { .. })));
    }

    #[test]
    fn rerank_candidate_limit_is_validated_when_setting_config() {
        let mut config = ShiroConfig::default();

        assert!(matches!(
            set_config_value(&mut config, "rerank.top_k", "0"),
            Err(ShiroError::InvalidInput { .. })
        ));
        assert_eq!(config.rerank, None);
    }

    #[test]
    fn automatic_concept_proposals_default_on_and_can_be_disabled() {
        let mut config = ShiroConfig::default();
        assert_eq!(
            get_config_value(&config, "ingest.auto_concept_proposals"),
            Some(toml::Value::Boolean(true))
        );

        set_config_value(&mut config, "ingest.auto_concept_proposals", "false").unwrap();
        assert_eq!(
            get_config_value(&config, "ingest.auto_concept_proposals"),
            Some(toml::Value::Boolean(false))
        );
    }

    #[test]
    fn sensitive_key_metadata_marks_api_key() {
        assert!(is_sensitive_key("embed.api_key"));
        assert!(!is_sensitive_key("embed.model"));
    }
}
