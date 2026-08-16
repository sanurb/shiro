//! `reindex` — rebuild derived indices (FTS, vector) from stored segments.

use serde::{Deserialize, Serialize};
use shiro_core::generation::{GenerationId, IndexGeneration};
use shiro_core::ports::Embedder;
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::{FlatIndex, FtsIndex};
use shiro_store::Store;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReindexOutput {
    pub index: String,
    pub status: String,
    pub documents: usize,
    pub segments: usize,
    pub generation: u64,
}

pub fn execute(home: &ShiroHome, store: &Store) -> Result<ReindexOutput, ShiroError> {
    // Collect all segments from READY documents.
    let docs = store.list_documents(100_000)?;
    let mut all_segments = Vec::new();
    let mut doc_count = 0usize;
    for (doc_id, state, _title) in &docs {
        if state.as_str() != "READY" {
            continue;
        }
        let segments = store.get_segments(doc_id)?;
        all_segments.extend(segments);
        doc_count += 1;
    }
    let seg_count = all_segments.len();

    // Derive next generation id from current active (or start at 1).
    let gen_id = store
        .active_generation("fts")
        .map(|g| g.next())
        .unwrap_or_else(|_| GenerationId::new(1));

    let gen_id_u64 = gen_id.as_u64();
    let created_at = utc_now_iso8601();

    let gen = IndexGeneration {
        gen_id: GenerationId::new(gen_id_u64),
        doc_count,
        segment_count: seg_count,
        created_at,
    };

    // Record generation before building.
    store.record_generation("fts", &gen)?;

    // Build into staging directory.
    let staging = home.staging_tantivy_dir();
    std::fs::create_dir_all(staging.as_std_path())?;
    FtsIndex::build_from_segments(&staging, &all_segments, gen_id_u64)?;

    // Promote staging → live.
    let live = home.tantivy_dir();
    FtsIndex::promote_staging(&staging, &live)?;

    // Set active generation.
    store.set_active_generation("fts", GenerationId::new(gen_id_u64))?;

    tracing::info!(
        doc_count,
        seg_count,
        gen_id = gen_id_u64,
        "FTS reindex complete"
    );

    Ok(ReindexOutput {
        index: "fts".into(),
        status: "rebuilt".into(),
        documents: doc_count,
        segments: seg_count,
        generation: gen_id_u64,
    })
}

/// Rebuild the vector index from Ready documents with the active embedding fingerprint.
pub fn execute_vector(
    home: &ShiroHome,
    store: &Store,
    embedder: &dyn Embedder,
) -> Result<ReindexOutput, ShiroError> {
    let docs = store.list_documents(100_000)?;
    let mut all_segments = Vec::new();
    let mut doc_count = 0usize;
    for (doc_id, state, _title) in &docs {
        if state.as_str() != "READY" {
            continue;
        }
        all_segments.extend(store.get_segments(doc_id)?);
        doc_count += 1;
    }
    let segment_count = all_segments.len();
    let generation = store
        .active_generation("vector")
        .map(|active| active.next())
        .unwrap_or_else(|_| GenerationId::new(1));
    let generation_value = generation.as_u64();

    store.record_generation(
        "vector",
        &IndexGeneration {
            gen_id: generation,
            doc_count,
            segment_count,
            created_at: utc_now_iso8601(),
        },
    )?;

    let texts: Vec<&str> = all_segments
        .iter()
        .map(|segment| segment.body.as_str())
        .collect();
    let embeddings = embedder.embed_batch(&texts)?;
    let entries: Vec<(String, String, Vec<f32>)> = all_segments
        .iter()
        .zip(embeddings)
        .map(|(segment, embedding)| {
            (
                segment.id.as_str().to_string(),
                segment.doc_id.as_str().to_string(),
                embedding,
            )
        })
        .collect();

    let staging_dir = home.staging_vector_dir();
    std::fs::create_dir_all(staging_dir.as_std_path())?;
    let staging_file = staging_dir.join("flat.jsonl");
    FlatIndex::build_at_with_fingerprint(
        embedder.dimensions(),
        staging_file.clone(),
        &entries,
        generation_value,
        &embedder.fingerprint(),
    )?;

    let live_dir = home.vector_dir();
    std::fs::create_dir_all(live_dir.as_std_path())?;
    FlatIndex::promote_staging(&staging_file, &live_dir.join("flat.jsonl"))?;
    store.set_active_generation("vector", generation)?;

    tracing::info!(
        doc_count,
        segment_count,
        gen_id = generation_value,
        dims = embedder.dimensions(),
        "vector reindex complete"
    );

    Ok(ReindexOutput {
        index: "vector".to_string(),
        status: "rebuilt".to_string(),
        documents: doc_count,
        segments: segment_count,
        generation: generation_value,
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
