use std::collections::HashMap;
use std::io::Write;
use std::sync::RwLock;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use shiro_core::fingerprint::EmbeddingFingerprint;
use shiro_core::ports::{VectorHit, VectorIndex};
use shiro_core::{DocId, SegmentId, ShiroError};

/// A single stored entry in the flat index (JSONL serialization shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    id: String,
    doc_id: String,
    vec: Vec<f32>,
}

/// In-memory vector entry.
struct VectorEntry {
    segment_id: SegmentId,
    doc_id: DocId,
    embedding: Vec<f32>,
}

/// Brute-force cosine-similarity vector index.
///
/// Stores all vectors in memory and persists to a JSONL file on [`VectorIndex::flush`].
/// This is a correctness baseline — not optimised for large-scale workloads.
pub struct FlatIndex {
    dims: usize,
    data_path: Utf8PathBuf,
    entries: RwLock<HashMap<String, VectorEntry>>,
    gen_id: u64,
    /// Blake3 checksum of the JSONL data computed on last flush.
    checksum: RwLock<Option<String>>,
    /// Fingerprint sidecar for ADR-012 embedding provenance tracking.
    stored_fingerprint: RwLock<Option<EmbeddingFingerprint>>,
}

impl FlatIndex {
    /// Open (or create) a flat index backed by the given JSONL file.
    ///
    /// If the file exists, entries are loaded. Malformed lines are logged and
    /// skipped — the index starts with whatever could be parsed.
    pub fn open(dims: usize, data_path: Utf8PathBuf) -> Result<Self, ShiroError> {
        let mut entries = HashMap::new();

        if data_path.as_std_path().is_file() {
            let content = std::fs::read_to_string(data_path.as_std_path()).map_err(|e| {
                ShiroError::IndexBuildVec {
                    message: format!("failed to read index file {data_path}: {e}"),
                }
            })?;

            for (lineno, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<StoredEntry>(line) {
                    Ok(stored) => {
                        let seg_id = match SegmentId::from_stored(&stored.id) {
                            Ok(id) => id,
                            Err(msg) => {
                                tracing::warn!(
                                    "{}:{}: invalid segment id '{}': {}",
                                    data_path,
                                    lineno + 1,
                                    stored.id,
                                    msg
                                );
                                continue;
                            }
                        };
                        let doc_id = match DocId::from_stored(&stored.doc_id) {
                            Ok(id) => id,
                            Err(msg) => {
                                tracing::warn!(
                                    "{}:{}: invalid doc id '{}': {}",
                                    data_path,
                                    lineno + 1,
                                    stored.doc_id,
                                    msg
                                );
                                continue;
                            }
                        };
                        if stored.vec.len() != dims {
                            tracing::warn!(
                                "{}:{}: dimension mismatch (expected {}, got {}), skipping",
                                data_path,
                                lineno + 1,
                                dims,
                                stored.vec.len()
                            );
                            continue;
                        }
                        entries.insert(
                            stored.id,
                            VectorEntry {
                                segment_id: seg_id,
                                doc_id,
                                embedding: stored.vec,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "{}:{}: malformed JSONL line, skipping: {}",
                            data_path,
                            lineno + 1,
                            e
                        );
                    }
                }
            }
        }

        // Load fingerprint sidecar if present.
        let fp_path = fingerprint_path(&data_path);
        let stored_fingerprint = if fp_path.as_std_path().is_file() {
            match std::fs::read_to_string(fp_path.as_std_path()) {
                Ok(json) => match serde_json::from_str::<EmbeddingFingerprint>(&json) {
                    Ok(fp) => Some(fp),
                    Err(e) => {
                        tracing::warn!("malformed fingerprint sidecar {fp_path}: {e}");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("failed to read fingerprint sidecar {fp_path}: {e}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            dims,
            data_path,
            entries: RwLock::new(entries),
            gen_id: 0,
            checksum: RwLock::new(None),
            stored_fingerprint: RwLock::new(stored_fingerprint),
        })
    }

    /// Return a clone of the stored fingerprint, if any.
    pub fn stored_fingerprint(&self) -> Option<EmbeddingFingerprint> {
        self.stored_fingerprint.read().ok().and_then(|g| g.clone())
    }

    /// Set the fingerprint for this index.
    ///
    /// If a fingerprint is already stored and differs, returns
    /// `ShiroError::FingerprintMismatch`.
    pub fn set_fingerprint(&self, fp: &EmbeddingFingerprint) -> Result<(), ShiroError> {
        let mut guard = self
            .stored_fingerprint
            .write()
            .map_err(|e| ShiroError::IndexBuildVec {
                message: format!("RwLock poisoned: {e}"),
            })?;
        if let Some(ref existing) = *guard {
            if existing.fingerprint_hash != fp.fingerprint_hash {
                return Err(ShiroError::FingerprintMismatch {
                    message: format!(
                        "index fingerprint mismatch: stored {} vs incoming {}",
                        existing.fingerprint_hash, fp.fingerprint_hash
                    ),
                });
            }
            // Already matches — no-op.
            return Ok(());
        }
        // Persist sidecar.
        let fp_path = fingerprint_path(&self.data_path);
        let json = serde_json::to_string_pretty(fp).map_err(|e| ShiroError::IndexBuildVec {
            message: format!("failed to serialize fingerprint: {e}"),
        })?;
        std::fs::write(fp_path.as_std_path(), json.as_bytes()).map_err(|e| {
            ShiroError::IndexBuildVec {
                message: format!("failed to write fingerprint sidecar {fp_path}: {e}"),
            }
        })?;
        *guard = Some(fp.clone());
        Ok(())
    }

    /// Insert or replace an embedding with an explicit [`DocId`].
    ///
    /// This is the preferred insertion method — it records which document owns
    /// the segment so that [`VectorIndex::delete_by_doc`] works correctly.
    pub fn upsert_with_doc(
        &self,
        segment_id: &SegmentId,
        doc_id: &DocId,
        embedding: &[f32],
    ) -> Result<(), ShiroError> {
        if embedding.len() != self.dims {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "dimension mismatch: expected {}, got {}",
                    self.dims,
                    embedding.len()
                ),
            });
        }
        let mut entries = self
            .entries
            .write()
            .map_err(|e| ShiroError::IndexBuildVec {
                message: format!("RwLock poisoned: {e}"),
            })?;
        entries.insert(
            segment_id.as_str().to_string(),
            VectorEntry {
                segment_id: segment_id.clone(),
                doc_id: doc_id.clone(),
                embedding: embedding.to_vec(),
            },
        );
        Ok(())
    }

    /// Return the generation ID.
    pub fn gen_id(&self) -> u64 {
        self.gen_id
    }

    /// Return the blake3 checksum computed on the last [`VectorIndex::flush`].
    pub fn checksum(&self) -> Option<String> {
        self.checksum.read().ok().and_then(|g| g.clone())
    }

    /// Verify that the on-disk JSONL file matches the stored checksum.
    pub fn verify_checksum(&self) -> Result<bool, ShiroError> {
        let stored = self
            .checksum
            .read()
            .map_err(|e| ShiroError::IndexBuildVec {
                message: format!("RwLock poisoned: {e}"),
            })?;
        let expected = match stored.as_deref() {
            Some(c) => c,
            None => return Ok(false),
        };
        let bytes =
            std::fs::read(self.data_path.as_std_path()).map_err(|e| ShiroError::IndexBuildVec {
                message: format!("failed to read {}: {e}", self.data_path),
            })?;
        let actual = blake3::hash(&bytes).to_hex().to_string();
        Ok(actual == expected)
    }

    /// Build a new index from entries at a given file path.
    ///
    /// The file is written and flushed immediately.
    pub fn build_at(
        dims: usize,
        data_path: Utf8PathBuf,
        entries: &[(String, String, Vec<f32>)],
        gen_id: u64,
    ) -> Result<Self, ShiroError> {
        let index = Self {
            dims,
            data_path,
            entries: RwLock::new(HashMap::new()),
            gen_id,
            checksum: RwLock::new(None),
            stored_fingerprint: RwLock::new(None),
        };
        for (seg_id, doc_id, embedding) in entries {
            let seg =
                SegmentId::from_stored(seg_id.clone()).map_err(|e| ShiroError::InvalidInput {
                    message: e.to_string(),
                })?;
            let did = DocId::from_stored(doc_id.clone()).map_err(|e| ShiroError::InvalidInput {
                message: e.to_string(),
            })?;
            index.upsert_with_doc(&seg, &did, embedding)?;
        }
        index.flush()?;
        Ok(index)
    }

    /// Atomic rename from staging file to live file.
    ///
    /// If the live path already exists, it is removed first.
    pub fn promote_staging(staging: &Utf8Path, live: &Utf8Path) -> Result<(), ShiroError> {
        if live.as_std_path().exists() {
            std::fs::remove_file(live.as_std_path()).map_err(|e| ShiroError::IndexBuildVec {
                message: format!("failed to remove live file {live}: {e}"),
            })?;
        }
        std::fs::rename(staging.as_std_path(), live.as_std_path()).map_err(|e| {
            ShiroError::IndexBuildVec {
                message: format!("failed to rename {staging} -> {live}: {e}"),
            }
        })?;
        // Promote fingerprint sidecar if present.
        let staging_fp = fingerprint_path(staging);
        let live_fp = fingerprint_path(live);
        if staging_fp.as_std_path().is_file() {
            if live_fp.as_std_path().exists() {
                std::fs::remove_file(live_fp.as_std_path()).map_err(|e| {
                    ShiroError::IndexBuildVec {
                        message: format!("failed to remove live fingerprint {live_fp}: {e}"),
                    }
                })?;
            }
            std::fs::rename(staging_fp.as_std_path(), live_fp.as_std_path()).map_err(|e| {
                ShiroError::IndexBuildVec {
                    message: format!("failed to rename fingerprint {staging_fp} -> {live_fp}: {e}"),
                }
            })?;
        }
        Ok(())
    }
}

fn fingerprint_path(data_path: &Utf8Path) -> Utf8PathBuf {
    let stem = data_path.file_stem().unwrap_or("flat");
    data_path.with_file_name(format!("{stem}.fingerprint.json"))
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

impl VectorIndex for FlatIndex {
    /// Insert or replace an embedding.
    ///
    /// Because the trait signature lacks a `doc_id` parameter, entries inserted
    /// via this method use a placeholder `doc_unknown` document ID. Prefer
    /// [`FlatIndex::upsert_with_doc`] when the document ID is available.
    fn upsert(&self, id: &SegmentId, embedding: &[f32]) -> Result<(), ShiroError> {
        if embedding.len() != self.dims {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "dimension mismatch: expected {}, got {}",
                    self.dims,
                    embedding.len()
                ),
            });
        }
        let placeholder_doc =
            DocId::from_stored("doc_unknown").map_err(|e| ShiroError::IndexBuildVec {
                message: e.to_string(),
            })?;
        let mut entries = self
            .entries
            .write()
            .map_err(|e| ShiroError::IndexBuildVec {
                message: format!("RwLock poisoned: {e}"),
            })?;
        entries.insert(
            id.as_str().to_string(),
            VectorEntry {
                segment_id: id.clone(),
                doc_id: placeholder_doc,
                embedding: embedding.to_vec(),
            },
        );
        Ok(())
    }

    fn delete(&self, id: &SegmentId) -> Result<(), ShiroError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| ShiroError::IndexBuildVec {
                message: format!("RwLock poisoned: {e}"),
            })?;
        entries.remove(id.as_str());
        Ok(())
    }

    fn delete_by_doc(&self, doc_id: &DocId) -> Result<(), ShiroError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| ShiroError::IndexBuildVec {
                message: format!("RwLock poisoned: {e}"),
            })?;
        entries.retain(|_, entry| entry.doc_id.as_str() != doc_id.as_str());
        Ok(())
    }

    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorHit>, ShiroError> {
        let entries = self.entries.read().map_err(|e| ShiroError::SearchFailed {
            message: format!("RwLock poisoned: {e}"),
        })?;

        let mut scored: Vec<(f32, &str)> = entries
            .values()
            .map(|entry| {
                let score = cosine_similarity(query, &entry.embedding);
                (score, entry.segment_id.as_str())
            })
            .collect();

        // Sort by score descending, then segment_id ascending for stable tie-break.
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(b.1))
        });

        scored.truncate(limit);

        scored
            .into_iter()
            .map(|(score, seg_str)| {
                let segment_id =
                    SegmentId::from_stored(seg_str).map_err(|e| ShiroError::IndexBuildVec {
                        message: e.to_string(),
                    })?;
                Ok(VectorHit { segment_id, score })
            })
            .collect()
    }

    fn count(&self) -> Result<usize, ShiroError> {
        let entries = self.entries.read().map_err(|e| ShiroError::IndexBuildVec {
            message: format!("RwLock poisoned: {e}"),
        })?;
        Ok(entries.len())
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn flush(&self) -> Result<(), ShiroError> {
        let entries = self.entries.read().map_err(|e| ShiroError::IndexBuildVec {
            message: format!("RwLock poisoned: {e}"),
        })?;

        let tmp_path = Utf8PathBuf::from(format!("{}.tmp", self.data_path));

        let mut buf = Vec::new();
        for entry in entries.values() {
            let stored = StoredEntry {
                id: entry.segment_id.as_str().to_string(),
                doc_id: entry.doc_id.as_str().to_string(),
                vec: entry.embedding.clone(),
            };
            let line = serde_json::to_string(&stored).map_err(|e| ShiroError::IndexBuildVec {
                message: format!("failed to serialize entry: {e}"),
            })?;
            buf.write_all(line.as_bytes()).unwrap();
            buf.write_all(b"\n").unwrap();
        }

        std::fs::write(tmp_path.as_std_path(), &buf).map_err(|e| ShiroError::IndexBuildVec {
            message: format!("failed to write tmp file {tmp_path}: {e}"),
        })?;

        std::fs::rename(tmp_path.as_std_path(), self.data_path.as_std_path()).map_err(|e| {
            ShiroError::IndexBuildVec {
                message: format!("failed to rename tmp to {}: {e}", self.data_path),
            }
        })?;

        // Compute blake3 checksum of the flushed file.
        let hash = blake3::hash(&buf).to_hex().to_string();
        if let Ok(mut guard) = self.checksum.write() {
            *guard = Some(hash);
        }

        // Persist fingerprint sidecar if one is set.
        if let Ok(guard) = self.stored_fingerprint.read() {
            if let Some(ref fp) = *guard {
                let fp_path = fingerprint_path(&self.data_path);
                let json =
                    serde_json::to_string_pretty(fp).map_err(|e| ShiroError::IndexBuildVec {
                        message: format!("failed to serialize fingerprint: {e}"),
                    })?;
                std::fs::write(fp_path.as_std_path(), json.as_bytes()).map_err(|e| {
                    ShiroError::IndexBuildVec {
                        message: format!("failed to write fingerprint sidecar {fp_path}: {e}"),
                    }
                })?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::ports::VectorIndex;
    use tempfile::TempDir;

    fn test_path(dir: &TempDir) -> Utf8PathBuf {
        let p = dir.path().join("index.jsonl");
        Utf8PathBuf::try_from(p.to_path_buf()).expect("tempdir path should be valid UTF-8")
    }

    fn seg(name: &str) -> SegmentId {
        SegmentId::from_stored(format!("seg_{name}")).unwrap()
    }

    fn doc(name: &str) -> DocId {
        DocId::from_stored(format!("doc_{name}")).unwrap()
    }

    #[test]
    fn test_upsert_and_search() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(3, test_path(&dir)).unwrap();

        idx.upsert(&seg("a"), &[1.0, 0.0, 0.0]).unwrap();
        idx.upsert(&seg("b"), &[0.0, 1.0, 0.0]).unwrap();
        idx.upsert(&seg("c"), &[1.0, 1.0, 0.0]).unwrap();

        let results = idx.search(&[1.0, 0.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 3);
        // seg_a should be most similar to [1,0,0]
        assert_eq!(results[0].segment_id.as_str(), "seg_a");
        assert!((results[0].score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_delete() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();

        idx.upsert(&seg("x"), &[1.0, 0.0]).unwrap();
        idx.upsert(&seg("y"), &[0.0, 1.0]).unwrap();
        assert_eq!(idx.count().unwrap(), 2);

        idx.delete(&seg("x")).unwrap();
        assert_eq!(idx.count().unwrap(), 1);

        let results = idx.search(&[1.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].segment_id.as_str(), "seg_y");
    }

    #[test]
    fn test_delete_by_doc() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();

        let d1 = doc("one");
        let d2 = doc("two");

        idx.upsert_with_doc(&seg("a1"), &d1, &[1.0, 0.0]).unwrap();
        idx.upsert_with_doc(&seg("a2"), &d1, &[0.0, 1.0]).unwrap();
        idx.upsert_with_doc(&seg("b1"), &d2, &[1.0, 1.0]).unwrap();

        assert_eq!(idx.count().unwrap(), 3);
        idx.delete_by_doc(&d1).unwrap();
        assert_eq!(idx.count().unwrap(), 1);

        let results = idx.search(&[1.0, 1.0], 10).unwrap();
        assert_eq!(results[0].segment_id.as_str(), "seg_b1");
    }

    #[test]
    fn test_flush_and_reload() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);

        {
            let idx = FlatIndex::open(2, path.clone()).unwrap();
            idx.upsert_with_doc(&seg("p"), &doc("d1"), &[0.5, 0.5])
                .unwrap();
            idx.upsert_with_doc(&seg("q"), &doc("d2"), &[1.0, 0.0])
                .unwrap();
            idx.flush().unwrap();
        }

        // Reload from same file
        let idx2 = FlatIndex::open(2, path).unwrap();
        assert_eq!(idx2.count().unwrap(), 2);

        let results = idx2.search(&[1.0, 0.0], 1).unwrap();
        assert_eq!(results[0].segment_id.as_str(), "seg_q");
        assert!((results[0].score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_dimension_mismatch() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(3, test_path(&dir)).unwrap();

        let result = idx.upsert(&seg("bad"), &[1.0, 2.0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cosine_similarity_correctness() {
        // cos([1,0], [0,1]) = 0
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // cos([1,0], [1,0]) = 1
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        // cos([1,1], [1,0]) = 1/sqrt(2) ≈ 0.7071
        let sim = cosine_similarity(&[1.0, 1.0], &[1.0, 0.0]);
        assert!((sim - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        // cos([3,4], [4,3]) = (12+12)/(5*5) = 24/25 = 0.96
        let sim2 = cosine_similarity(&[3.0, 4.0], &[4.0, 3.0]);
        assert!((sim2 - 0.96).abs() < 1e-5);
    }

    #[test]
    fn test_zero_vector_handling() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();

        idx.upsert(&seg("z"), &[0.0, 0.0]).unwrap();
        let results = idx.search(&[0.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.0);

        // Search with non-zero query against zero vector
        let results2 = idx.search(&[1.0, 0.0], 10).unwrap();
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].score, 0.0);
    }

    #[test]
    fn test_stable_tiebreak() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();

        // All orthogonal to query — same score (0.0)
        idx.upsert(&seg("ccc"), &[0.0, 1.0]).unwrap();
        idx.upsert(&seg("aaa"), &[0.0, 1.0]).unwrap();
        idx.upsert(&seg("bbb"), &[0.0, 1.0]).unwrap();

        let results = idx.search(&[1.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 3);
        // Tied scores → lexicographic ascending on segment_id
        assert_eq!(results[0].segment_id.as_str(), "seg_aaa");
        assert_eq!(results[1].segment_id.as_str(), "seg_bbb");
        assert_eq!(results[2].segment_id.as_str(), "seg_ccc");
    }

    #[test]
    fn test_empty_search() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(3, test_path(&dir)).unwrap();

        let results = idx.search(&[1.0, 0.0, 0.0], 10).unwrap();
        assert!(results.is_empty());
        assert_eq!(idx.count().unwrap(), 0);
    }

    #[test]
    fn test_gen_id_default() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();
        assert_eq!(idx.gen_id(), 0);
    }

    #[test]
    fn test_build_at_with_entries() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);
        let entries = vec![
            ("seg_a".to_string(), "doc_one".to_string(), vec![1.0, 0.0]),
            ("seg_b".to_string(), "doc_two".to_string(), vec![0.0, 1.0]),
        ];
        let idx = FlatIndex::build_at(2, path.clone(), &entries, 42).unwrap();
        assert_eq!(idx.gen_id(), 42);
        assert_eq!(idx.count().unwrap(), 2);

        // Data persisted — reload
        let idx2 = FlatIndex::open(2, path).unwrap();
        assert_eq!(idx2.count().unwrap(), 2);
        let results = idx2.search(&[1.0, 0.0], 1).unwrap();
        assert_eq!(results[0].segment_id.as_str(), "seg_a");
    }

    #[test]
    fn test_build_at_empty() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);
        let idx = FlatIndex::build_at(3, path.clone(), &[], 1).unwrap();
        assert_eq!(idx.gen_id(), 1);
        assert_eq!(idx.count().unwrap(), 0);
        assert!(path.as_std_path().exists());
    }

    #[test]
    fn test_promote_staging() {
        let dir = TempDir::new().unwrap();
        let staging = test_path_named(&dir, "staging.jsonl");
        let live = test_path_named(&dir, "live.jsonl");

        // Build at staging path
        let entries = vec![(
            "seg_x".to_string(),
            "doc_d".to_string(),
            vec![1.0, 0.0, 0.0],
        )];
        FlatIndex::build_at(3, staging.clone(), &entries, 5).unwrap();
        assert!(staging.as_std_path().exists());

        FlatIndex::promote_staging(&staging, &live).unwrap();
        assert!(!staging.as_std_path().exists());
        assert!(live.as_std_path().exists());

        let idx = FlatIndex::open(3, live).unwrap();
        assert_eq!(idx.count().unwrap(), 1);
    }

    #[test]
    fn test_promote_staging_overwrites_live() {
        let dir = TempDir::new().unwrap();
        let staging = test_path_named(&dir, "staging.jsonl");
        let live = test_path_named(&dir, "live.jsonl");

        // Create live with old data
        let old = vec![("seg_old".to_string(), "doc_o".to_string(), vec![0.0, 1.0])];
        FlatIndex::build_at(2, live.clone(), &old, 1).unwrap();

        // Create staging with new data
        let new_entries = vec![("seg_new".to_string(), "doc_n".to_string(), vec![1.0, 0.0])];
        FlatIndex::build_at(2, staging.clone(), &new_entries, 2).unwrap();

        FlatIndex::promote_staging(&staging, &live).unwrap();
        let idx = FlatIndex::open(2, live).unwrap();
        assert_eq!(idx.count().unwrap(), 1);
        let r = idx.search(&[1.0, 0.0], 1).unwrap();
        assert_eq!(r[0].segment_id.as_str(), "seg_new");
    }

    #[test]
    fn test_checksum_after_flush() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();
        assert!(idx.checksum().is_none());

        idx.upsert(&seg("a"), &[1.0, 0.0]).unwrap();
        idx.flush().unwrap();

        let cs = idx.checksum();
        assert!(cs.is_some());
        // Blake3 hex is 64 chars
        assert_eq!(cs.as_ref().unwrap().len(), 64);

        assert!(idx.verify_checksum().unwrap());
    }

    #[test]
    fn test_checksum_empty_index() {
        let dir = TempDir::new().unwrap();
        let idx = FlatIndex::open(2, test_path(&dir)).unwrap();
        idx.flush().unwrap();

        assert!(idx.checksum().is_some());
        assert!(idx.verify_checksum().unwrap());
    }

    #[test]
    fn test_checksum_detects_tampering() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);
        let idx = FlatIndex::open(2, path.clone()).unwrap();
        idx.upsert(&seg("a"), &[1.0, 0.0]).unwrap();
        idx.flush().unwrap();
        assert!(idx.verify_checksum().unwrap());

        // Tamper with the file
        std::fs::write(path.as_std_path(), b"tampered").unwrap();
        assert!(!idx.verify_checksum().unwrap());
    }

    fn test_path_named(dir: &TempDir, name: &str) -> Utf8PathBuf {
        let p = dir.path().join(name);
        Utf8PathBuf::try_from(p.to_path_buf()).expect("tempdir path should be valid UTF-8")
    }

    #[test]
    fn fingerprint_sidecar_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);

        let fp = EmbeddingFingerprint::new(
            "test".to_string(),
            "model-a".to_string(),
            384,
            "l2".to_string(),
            "none".to_string(),
            "full_segment".to_string(),
        );

        {
            let idx = FlatIndex::open(384, path.clone()).unwrap();
            idx.set_fingerprint(&fp).unwrap();
            idx.flush().unwrap();
        }

        let idx2 = FlatIndex::open(384, path).unwrap();
        let stored = idx2
            .stored_fingerprint()
            .expect("fingerprint should persist");
        assert_eq!(stored.fingerprint_hash, fp.fingerprint_hash);
    }

    #[test]
    fn fingerprint_mismatch_is_hard_error() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);

        let fp_a = EmbeddingFingerprint::new(
            "test".to_string(),
            "model-a".to_string(),
            384,
            "l2".to_string(),
            "none".to_string(),
            "full_segment".to_string(),
        );
        let fp_b = EmbeddingFingerprint::new(
            "test".to_string(),
            "model-b".to_string(),
            768,
            "l2".to_string(),
            "none".to_string(),
            "full_segment".to_string(),
        );

        let idx = FlatIndex::open(384, path).unwrap();
        idx.set_fingerprint(&fp_a).unwrap();
        idx.flush().unwrap();

        let result = idx.set_fingerprint(&fp_b);
        assert!(matches!(
            result,
            Err(ShiroError::FingerprintMismatch { .. })
        ));
    }

    #[test]
    fn fingerprint_same_is_noop() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);

        let fp = EmbeddingFingerprint::new(
            "test".to_string(),
            "model-a".to_string(),
            384,
            "l2".to_string(),
            "none".to_string(),
            "full_segment".to_string(),
        );

        let idx = FlatIndex::open(384, path).unwrap();
        idx.set_fingerprint(&fp).unwrap();
        idx.set_fingerprint(&fp).unwrap(); // second call should succeed
    }

    #[test]
    fn fresh_index_has_no_fingerprint() {
        let dir = TempDir::new().unwrap();
        let path = test_path(&dir);

        let idx = FlatIndex::open(384, path).unwrap();
        assert!(idx.stored_fingerprint().is_none());
    }
}
