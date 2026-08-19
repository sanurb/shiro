//! `reindex` — rebuild derived indices (FTS, vector) from stored segments.

use serde::{Deserialize, Serialize};
use shiro_core::ports::Embedder;
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

use super::corpus_publication;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReindexOutput {
    pub index: String,
    pub status: String,
    pub documents: usize,
    pub segments: usize,
    pub generation: u64,
}

pub fn execute(home: &ShiroHome, store: &Store) -> Result<ReindexOutput, ShiroError> {
    corpus_publication::publish_all(home, store, None)?
        .into_iter()
        .next()
        .ok_or_else(|| ShiroError::IndexBuildFts {
            message: "corpus publication produced no FTS generation".to_string(),
        })
}

/// Rebuild every configured derived index and activate one common manifest.
pub fn execute_all(
    home: &ShiroHome,
    store: &Store,
    embedder: Option<&dyn Embedder>,
) -> Result<Vec<ReindexOutput>, ShiroError> {
    corpus_publication::publish_all(home, store, embedder)
}

/// Rebuild the vector index from Ready documents with the active embedding fingerprint.
pub fn execute_vector(
    home: &ShiroHome,
    store: &Store,
    embedder: &dyn Embedder,
) -> Result<ReindexOutput, ShiroError> {
    execute_all(home, store, Some(embedder))?
        .into_iter()
        .find(|output| output.index == "vector")
        .ok_or_else(|| ShiroError::IndexBuildVec {
            message: "corpus publication produced no vector generation".to_string(),
        })
}

// ---------------------------------------------------------------------------
// Time helpers (pub(crate) for potential reuse)
// ---------------------------------------------------------------------------

/// Returns a minimal ISO 8601 UTC timestamp (seconds precision) without external crates.
pub(crate) fn utc_now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = epoch_secs_to_parts(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Decompose a Unix timestamp (seconds) into (year, month, day, hour, min, sec) UTC.
/// Uses the Gregorian calendar algorithm; valid for dates 1970–2099.
pub(crate) fn epoch_secs_to_parts(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let s = (secs % 60) as u32;
    let mins = secs / 60;
    let mi = (mins % 60) as u32;
    let hours = mins / 60;
    let h = (hours % 24) as u32;
    let days = (hours / 24) as u32;

    // Days since 1970-01-01
    let mut y = 1970u32;
    let mut d = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let months = [
        31,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1u32;
    for &dim in &months {
        if d < dim {
            break;
        }
        d -= dim;
        mo += 1;
    }
    (y, mo, d + 1, h, mi, s)
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
