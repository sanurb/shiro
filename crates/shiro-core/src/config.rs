//! Configuration types and `ShiroHome` path management.
//!
//! `ShiroHome` resolves all derived paths (DB, indices, config) from a
//! single root directory. Default: `~/.shiro`.

use camino::{Utf8Path, Utf8PathBuf};

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

    pub fn config_path(&self) -> Utf8PathBuf {
        self.root.join("config.toml")
    }

    pub fn lock_dir(&self) -> Utf8PathBuf {
        self.root.join("lock")
    }

    /// Create the directory structure if it does not exist.
    pub fn ensure_dirs(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(self.root.as_std_path())?;
        std::fs::create_dir_all(self.tantivy_dir().as_std_path())?;
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
}
