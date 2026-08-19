//! Recoverable publication of complete, immutable derived-index generations.
//!
//! Builders write generation-specific artifacts that are never live while
//! incomplete. One SQLite manifest activation makes the complete FTS/vector
//! view authoritative; orphaned failed generations are harmless and auditable.

use shiro_core::generation::{CorpusManifest, GenerationId};
use shiro_core::ir::Segment;
use shiro_core::ports::Embedder;
use shiro_core::{DocId, ShiroError, ShiroHome};
use shiro_index::{artifact_digest, FlatIndex, FtsIndex};
use shiro_store::Store;

use super::reindex::{utc_now_iso8601, ReindexOutput};

#[derive(Debug)]
struct CorpusSnapshot {
    document_count: usize,
    segments: Vec<Segment>,
    digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicationCheckpoint {
    FtsGenerationReserved,
    FtsArtifactValidated,
    VectorGenerationReserved,
    VectorArtifactValidated,
    BeforeManifestActivation,
    AfterManifestActivation,
}

pub(crate) fn publish_all(
    home: &ShiroHome,
    store: &Store,
    embedder: Option<&dyn Embedder>,
) -> Result<Vec<ReindexOutput>, ShiroError> {
    publish_all_with_checkpoint(home, store, embedder, |_| Ok(()))
}

pub(crate) fn publish_all_with_checkpoint<F>(
    home: &ShiroHome,
    store: &Store,
    embedder: Option<&dyn Embedder>,
    mut checkpoint: F,
) -> Result<Vec<ReindexOutput>, ShiroError>
where
    F: FnMut(PublicationCheckpoint) -> Result<(), ShiroError>,
{
    let snapshot = collect_ready_corpus(store)?;
    let created_at = utc_now_iso8601();
    let fts_generation = store.reserve_generation(
        "fts",
        snapshot.document_count,
        snapshot.segments.len(),
        &created_at,
    )?;
    checkpoint(PublicationCheckpoint::FtsGenerationReserved)?;

    let fts_digest = build_and_validate_fts(home, &snapshot, fts_generation)?;
    checkpoint(PublicationCheckpoint::FtsArtifactValidated)?;

    let vector_artifact = match embedder {
        Some(embedder) => {
            let generation = store.reserve_generation(
                "vector",
                snapshot.document_count,
                snapshot.segments.len(),
                &created_at,
            )?;
            checkpoint(PublicationCheckpoint::VectorGenerationReserved)?;
            let digest = build_and_validate_vector(home, &snapshot, generation, embedder)?;
            checkpoint(PublicationCheckpoint::VectorArtifactValidated)?;
            Some((
                generation,
                digest,
                crate::retrieval_embedding_fingerprint(&embedder.fingerprint()).fingerprint_hash,
            ))
        }
        None => None,
    };

    let manifest = make_manifest(
        &snapshot,
        created_at,
        fts_generation,
        fts_digest,
        vector_artifact.as_ref(),
    );
    checkpoint(PublicationCheckpoint::BeforeManifestActivation)?;
    store.activate_corpus_manifest(&manifest)?;
    checkpoint(PublicationCheckpoint::AfterManifestActivation)?;
    clean_orphaned_generation_artifacts_after_activation(home, store);

    let mut outputs = vec![ReindexOutput {
        index: "fts".to_string(),
        status: "rebuilt".to_string(),
        documents: snapshot.document_count,
        segments: snapshot.segments.len(),
        generation: fts_generation.as_u64(),
    }];
    if let Some((generation, _, _)) = vector_artifact {
        outputs.push(ReindexOutput {
            index: "vector".to_string(),
            status: "rebuilt".to_string(),
            documents: snapshot.document_count,
            segments: snapshot.segments.len(),
            generation: generation.as_u64(),
        });
    }
    Ok(outputs)
}

/// Result of a bounded incremental common-generation publication.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct IncrementalPublicationOutput {
    pub fts_generation: u64,
    pub vector_generation: u64,
    pub documents: usize,
    pub segments: usize,
    pub embedded_segments: usize,
    pub reused_segments: usize,
    pub embedding_batches: usize,
}

/// Never-reused generation IDs reserved before an atomic incremental publication begins.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReservedIncrementalGenerations {
    fts_generation: GenerationId,
    vector_generation: GenerationId,
}

/// Reserve generation IDs outside the publication transaction so failed artifacts are never reused.
pub(crate) fn reserve_incremental_generations(
    store: &Store,
) -> Result<ReservedIncrementalGenerations, ShiroError> {
    let created_at = utc_now_iso8601();
    Ok(ReservedIncrementalGenerations {
        fts_generation: store.reserve_generation("fts", 0, 0, &created_at)?,
        vector_generation: store.reserve_generation("vector", 0, 0, &created_at)?,
    })
}

/// Build and activate staged FTS/vector generations using newly reserved generation IDs.
#[cfg(test)]
pub(crate) fn publish_incremental_staged(
    home: &ShiroHome,
    store: &Store,
    embedder: &dyn Embedder,
    changed_document_ids: &[DocId],
    embedding_batch_size: usize,
) -> Result<IncrementalPublicationOutput, ShiroError> {
    let generations = reserve_incremental_generations(store)?;
    publish_reserved_incremental_staged(
        home,
        store,
        embedder,
        changed_document_ids,
        embedding_batch_size,
        generations,
    )
}

/// Build and atomically activate a complete generation while embedding only changed segments.
pub(crate) fn publish_reserved_incremental_staged(
    home: &ShiroHome,
    store: &Store,
    embedder: &dyn Embedder,
    changed_document_ids: &[DocId],
    embedding_batch_size: usize,
    generations: ReservedIncrementalGenerations,
) -> Result<IncrementalPublicationOutput, ShiroError> {
    if embedding_batch_size == 0 {
        return Err(ShiroError::InvalidInput {
            message: "embedding batch size must be positive".to_string(),
        });
    }
    let staged = changed_document_ids
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let snapshot = collect_corpus(store, &staged)?;
    let fts_generation = generations.fts_generation;
    let vector_generation = generations.vector_generation;
    store.update_reserved_generation_counts(
        "fts",
        fts_generation,
        snapshot.document_count,
        snapshot.segments.len(),
    )?;
    store.update_reserved_generation_counts(
        "vector",
        vector_generation,
        snapshot.document_count,
        snapshot.segments.len(),
    )?;
    let created_at = utc_now_iso8601();
    let fts_digest = build_and_validate_fts(home, &snapshot, fts_generation)?;

    let fingerprint = crate::retrieval_embedding_fingerprint(&embedder.fingerprint());
    let changed = changed_document_ids
        .iter()
        .map(DocId::as_str)
        .collect::<std::collections::HashSet<_>>();
    let mut reusable = load_reusable_vectors(home, store, embedder, &fingerprint)?;
    let mut entries = Vec::with_capacity(snapshot.segments.len());
    let mut pending = Vec::new();
    let mut reused_segments = 0usize;
    for segment in &snapshot.segments {
        if !changed.contains(segment.doc_id.as_str()) {
            if let Some((doc_id, vector)) = reusable.remove(segment.id.as_str()) {
                entries.push((segment.id.as_str().to_string(), doc_id, vector));
                reused_segments += 1;
                continue;
            }
        }
        pending.push(segment);
    }

    let mut embedding_batches = 0usize;
    for batch in pending.chunks(embedding_batch_size) {
        let texts = batch
            .iter()
            .map(|segment| segment.retrieval_text.as_str())
            .collect::<Vec<_>>();
        let embeddings = embedder.embed_batch(&texts)?;
        if embeddings.len() != batch.len() {
            return Err(ShiroError::EmbedFail {
                message: format!(
                    "embedder returned {} vectors for {} incremental segments",
                    embeddings.len(),
                    batch.len()
                ),
            });
        }
        for (segment, vector) in batch.iter().zip(embeddings) {
            if vector.len() != embedder.dimensions() {
                return Err(ShiroError::EmbedFail {
                    message: format!(
                        "embedder returned {} dimensions; expected {}",
                        vector.len(),
                        embedder.dimensions()
                    ),
                });
            }
            entries.push((
                segment.id.as_str().to_string(),
                segment.doc_id.as_str().to_string(),
                vector,
            ));
        }
        embedding_batches += 1;
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let vector_digest = build_and_validate_vector_entries(
        home,
        &snapshot,
        vector_generation,
        embedder,
        &fingerprint,
        &entries,
    )?;
    let vector_artifact = (
        vector_generation,
        vector_digest,
        fingerprint.fingerprint_hash.clone(),
    );
    let manifest = make_manifest(
        &snapshot,
        created_at,
        fts_generation,
        fts_digest,
        Some(&vector_artifact),
    );
    store.activate_corpus_manifest_and_ready(&manifest, changed_document_ids)?;
    clean_orphaned_generation_artifacts_after_activation(home, store);

    Ok(IncrementalPublicationOutput {
        fts_generation: fts_generation.as_u64(),
        vector_generation: vector_generation.as_u64(),
        documents: snapshot.document_count,
        segments: snapshot.segments.len(),
        embedded_segments: pending.len(),
        reused_segments,
        embedding_batches,
    })
}

fn load_reusable_vectors(
    home: &ShiroHome,
    store: &Store,
    embedder: &dyn Embedder,
    fingerprint: &shiro_core::EmbeddingFingerprint,
) -> Result<std::collections::HashMap<String, (String, Vec<f32>)>, ShiroError> {
    let Some(previous) = store.latest_vector_manifest(&fingerprint.fingerprint_hash)? else {
        return Ok(std::collections::HashMap::new());
    };
    let generation = previous
        .vector_generation
        .ok_or_else(|| ShiroError::StoreCorrupt {
            message: "vector manifest lacks a vector generation".to_string(),
        })?;
    let expected_digest = previous
        .vector_digest
        .ok_or_else(|| ShiroError::StoreCorrupt {
            message: "vector manifest lacks a vector digest".to_string(),
        })?;
    let directory = home.vector_generation_dir(generation.as_u64());
    if artifact_digest(&directory)? != expected_digest {
        return Err(ShiroError::IndexBuildVec {
            message: format!("reusable vector generation {generation} failed digest validation"),
        });
    }
    let index = FlatIndex::open_generation_compatible(
        embedder.dimensions(),
        home.vector_data_path(generation.as_u64()),
        generation.as_u64(),
        fingerprint,
    )?;
    Ok(index
        .snapshot_entries()?
        .into_iter()
        .map(|(segment_id, doc_id, vector)| (segment_id, (doc_id, vector)))
        .collect())
}

fn build_and_validate_vector_entries(
    home: &ShiroHome,
    snapshot: &CorpusSnapshot,
    generation: GenerationId,
    embedder: &dyn Embedder,
    fingerprint: &shiro_core::EmbeddingFingerprint,
    entries: &[(String, String, Vec<f32>)],
) -> Result<String, ShiroError> {
    if entries.len() != snapshot.segments.len() {
        return Err(ShiroError::IndexBuildVec {
            message: format!(
                "incremental vector generation has {} entries; expected {}",
                entries.len(),
                snapshot.segments.len()
            ),
        });
    }
    let directory = home.vector_generation_dir(generation.as_u64());
    std::fs::create_dir_all(directory.as_std_path())?;
    let data_path = home.vector_data_path(generation.as_u64());
    FlatIndex::build_at_with_fingerprint(
        embedder.dimensions(),
        data_path.clone(),
        entries,
        generation.as_u64(),
        fingerprint,
    )?;
    let reopened = FlatIndex::open_generation_compatible(
        embedder.dimensions(),
        data_path,
        generation.as_u64(),
        fingerprint,
    )?;
    if shiro_core::ports::VectorIndex::count(&reopened)? != snapshot.segments.len() {
        return Err(ShiroError::IndexBuildVec {
            message: "incremental vector generation failed complete-count validation".to_string(),
        });
    }
    artifact_digest(&directory)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct OrphanedGenerationArtifactCleanup {
    removed_fts_generations: Vec<GenerationId>,
    removed_vector_generations: Vec<GenerationId>,
}

/// Remove generation directories that no retained corpus manifest can reopen.
fn garbage_collect_orphaned_generation_artifacts(
    home: &ShiroHome,
    store: &Store,
) -> Result<OrphanedGenerationArtifactCleanup, ShiroError> {
    let references = store.corpus_manifest_generation_references()?;
    let protected_fts = references
        .fts_generations
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let protected_vectors = references
        .vector_generations
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    Ok(OrphanedGenerationArtifactCleanup {
        removed_fts_generations: remove_unreferenced_generation_directories(
            home,
            "tantivy_gen_",
            &protected_fts,
        )?,
        removed_vector_generations: remove_unreferenced_generation_directories(
            home,
            "vector_gen_",
            &protected_vectors,
        )?,
    })
}

fn remove_unreferenced_generation_directories(
    home: &ShiroHome,
    directory_prefix: &str,
    protected_generations: &std::collections::HashSet<GenerationId>,
) -> Result<Vec<GenerationId>, ShiroError> {
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(home.root().as_std_path())? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(generation) = parse_generation_directory_name(file_name, directory_prefix) else {
            continue;
        };
        if protected_generations.contains(&generation) {
            continue;
        }
        std::fs::remove_dir_all(entry.path())?;
        removed.push(generation);
    }
    removed.sort_unstable();
    Ok(removed)
}

fn parse_generation_directory_name(
    file_name: &str,
    directory_prefix: &str,
) -> Option<GenerationId> {
    let generation_text = file_name.strip_prefix(directory_prefix)?;
    let generation = generation_text.parse::<u64>().ok()?;
    if generation == 0 || generation.to_string() != generation_text {
        return None;
    }
    Some(GenerationId::new(generation))
}

fn clean_orphaned_generation_artifacts_after_activation(home: &ShiroHome, store: &Store) {
    match garbage_collect_orphaned_generation_artifacts(home, store) {
        Ok(cleanup)
            if !cleanup.removed_fts_generations.is_empty()
                || !cleanup.removed_vector_generations.is_empty() =>
        {
            tracing::info!(
                fts_generations = ?cleanup.removed_fts_generations,
                vector_generations = ?cleanup.removed_vector_generations,
                "orphaned generation artifact cleanup completed"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "orphaned generation artifact cleanup failed after activation");
        }
    }
}

pub(crate) fn verify_active_manifest_artifacts(
    home: &ShiroHome,
    store: &Store,
) -> Result<(), ShiroError> {
    let Some(manifest) = store.active_corpus_manifest()? else {
        return Ok(());
    };
    // Incremental FTS publication deliberately marks the active artifact as
    // mutable while deactivating vectors. A full rebuild restores a digest.
    if !manifest.fts_digest.is_empty() {
        let path = home.tantivy_generation_dir(manifest.fts_generation.as_u64());
        let actual = artifact_digest(&path)?;
        if actual != manifest.fts_digest {
            return Err(ShiroError::IndexBuildFts {
                message: format!(
                    "active FTS generation {} failed manifest digest verification",
                    manifest.fts_generation
                ),
            });
        }
    }
    if let (Some(generation), Some(expected_digest)) =
        (manifest.vector_generation, manifest.vector_digest)
    {
        let path = home.vector_generation_dir(generation.as_u64());
        let actual = artifact_digest(&path).map_err(|error| ShiroError::IndexBuildVec {
            message: format!("failed to verify active vector generation: {error}"),
        })?;
        if actual != expected_digest {
            return Err(ShiroError::IndexBuildVec {
                message: format!(
                    "active vector generation {generation} failed manifest digest verification"
                ),
            });
        }
    }
    Ok(())
}

fn collect_ready_corpus(store: &Store) -> Result<CorpusSnapshot, ShiroError> {
    collect_corpus(store, &std::collections::HashSet::new())
}

fn collect_corpus(
    store: &Store,
    staged_document_ids: &std::collections::HashSet<&DocId>,
) -> Result<CorpusSnapshot, ShiroError> {
    let mut documents = store.list_all_documents()?;
    documents.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
    let mut segments = Vec::new();
    let mut document_count = 0usize;
    for (doc_id, state, _) in documents {
        if state.as_str() != "READY" && !staged_document_ids.contains(&doc_id) {
            continue;
        }
        let mut document_segments = store.get_segments(&doc_id)?;
        document_segments.sort_by_key(|segment| segment.index);
        segments.extend(document_segments);
        document_count += 1;
    }

    let mut hasher = blake3::Hasher::new();
    for segment in &segments {
        hasher.update(segment.doc_id.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(segment.id.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(segment.body.as_bytes());
        hasher.update(b"\0");
        hasher.update(segment.retrieval_text.as_bytes());
        hasher.update(b"\0");
    }
    Ok(CorpusSnapshot {
        document_count,
        segments,
        digest: hasher.finalize().to_hex().to_string(),
    })
}

fn build_and_validate_fts(
    home: &ShiroHome,
    snapshot: &CorpusSnapshot,
    generation: GenerationId,
) -> Result<String, ShiroError> {
    let path = home.tantivy_generation_dir(generation.as_u64());
    FtsIndex::build_from_segments(&path, &snapshot.segments, generation.as_u64())?;
    let reopened = FtsIndex::open_generation(&path, generation.as_u64())?;
    let actual_count = reopened.num_segments()? as usize;
    if actual_count != snapshot.segments.len() {
        return Err(ShiroError::IndexBuildFts {
            message: format!(
                "FTS generation {} contains {actual_count} segments; expected {}",
                generation,
                snapshot.segments.len()
            ),
        });
    }
    artifact_digest(&path)
}

fn build_and_validate_vector(
    home: &ShiroHome,
    snapshot: &CorpusSnapshot,
    generation: GenerationId,
    embedder: &dyn Embedder,
) -> Result<String, ShiroError> {
    let texts: Vec<&str> = snapshot
        .segments
        .iter()
        .map(|segment| segment.retrieval_text.as_str())
        .collect();
    let embeddings = embedder.embed_batch(&texts)?;
    if embeddings.len() != snapshot.segments.len() {
        return Err(ShiroError::EmbedFail {
            message: format!(
                "embedder returned {} vectors for {} segments",
                embeddings.len(),
                snapshot.segments.len()
            ),
        });
    }
    if let Some(invalid) = embeddings
        .iter()
        .find(|embedding| embedding.len() != embedder.dimensions())
    {
        return Err(ShiroError::EmbedFail {
            message: format!(
                "embedder returned {} dimensions; expected {}",
                invalid.len(),
                embedder.dimensions()
            ),
        });
    }

    let entries: Vec<(String, String, Vec<f32>)> = snapshot
        .segments
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
    let directory = home.vector_generation_dir(generation.as_u64());
    if directory.as_std_path().exists() {
        std::fs::remove_dir_all(directory.as_std_path())?;
    }
    std::fs::create_dir_all(directory.as_std_path())?;
    let data_path = home.vector_data_path(generation.as_u64());
    let fingerprint = crate::retrieval_embedding_fingerprint(&embedder.fingerprint());
    FlatIndex::build_at_with_fingerprint(
        embedder.dimensions(),
        data_path.clone(),
        &entries,
        generation.as_u64(),
        &fingerprint,
    )?;
    let reopened = FlatIndex::open_generation_compatible(
        embedder.dimensions(),
        data_path,
        generation.as_u64(),
        &fingerprint,
    )?;
    if shiro_core::ports::VectorIndex::count(&reopened)? != snapshot.segments.len() {
        return Err(ShiroError::IndexBuildVec {
            message: format!(
                "vector generation {} does not contain the complete corpus",
                generation
            ),
        });
    }
    artifact_digest(&directory)
}

fn make_manifest(
    snapshot: &CorpusSnapshot,
    created_at: String,
    fts_generation: GenerationId,
    fts_digest: String,
    vector: Option<&(GenerationId, String, String)>,
) -> CorpusManifest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(snapshot.digest.as_bytes());
    hasher.update(&fts_generation.as_u64().to_le_bytes());
    hasher.update(fts_digest.as_bytes());
    if let Some((generation, digest, fingerprint)) = vector {
        hasher.update(&generation.as_u64().to_le_bytes());
        hasher.update(digest.as_bytes());
        hasher.update(fingerprint.as_bytes());
    }
    let manifest_id = format!("corpus_{}", hasher.finalize().to_hex());
    CorpusManifest {
        manifest_id,
        corpus_digest: snapshot.digest.clone(),
        document_count: snapshot.document_count,
        segment_count: snapshot.segments.len(),
        fts_generation,
        fts_digest,
        vector_generation: vector.map(|(generation, _, _)| *generation),
        vector_digest: vector.map(|(_, digest, _)| digest.clone()),
        embedding_fingerprint_hash: vector.map(|(_, _, fingerprint)| fingerprint.clone()),
        created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::document_ingestion::ingest_document_bytes;
    use shiro_embed::DeterministicStubEmbedder;
    use shiro_parse::PlainTextParser;

    fn populated_home() -> (tempfile::TempDir, ShiroHome, Store) {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temporary.path().join("home")).unwrap();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let store = Store::open(&home.db_path()).unwrap();
        let fts = FtsIndex::open(&home.tantivy_dir()).unwrap();
        ingest_document_bytes(
            &store,
            &fts,
            &PlainTextParser,
            "fixture.txt",
            b"recoverable corpus publication evidence",
        )
        .unwrap();
        (temporary, home, store)
    }

    #[test]
    fn common_manifest_activates_complete_fts_and_vector_generations() {
        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);

        let outputs = publish_all(&home, &store, Some(&embedder)).unwrap();
        assert_eq!(outputs.len(), 2);
        let manifest = store.active_corpus_manifest().unwrap().unwrap();
        assert_eq!(manifest.document_count, 1);
        assert_eq!(manifest.segment_count, 1);
        assert_eq!(
            store.active_generation("fts").unwrap(),
            manifest.fts_generation
        );
        assert_eq!(
            store.active_generation("vector").unwrap(),
            manifest.vector_generation.unwrap()
        );
        verify_active_manifest_artifacts(&home, &store).unwrap();
        assert_eq!(
            artifact_digest(
                &home.vector_generation_dir(manifest.vector_generation.unwrap().as_u64())
            )
            .unwrap(),
            manifest.vector_digest.clone().unwrap()
        );

        let vector_generation = manifest.vector_generation.unwrap().as_u64();
        let vector_index = FlatIndex::open_generation_compatible(
            8,
            home.vector_data_path(vector_generation),
            vector_generation,
            &crate::retrieval_embedding_fingerprint(&embedder.fingerprint()),
        )
        .unwrap();
        let engine = crate::Engine::open(home)
            .unwrap()
            .with_embedder(Box::new(embedder))
            .with_vector_index(Box::new(vector_index));
        let search = engine
            .search(&crate::ops::search::SearchInput {
                query: "publication evidence".to_string(),
                mode: crate::ops::search::SearchMode::Hybrid,
                limit: 5,
                expand: false,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: crate::ops::search::SearchFilters::default(),
            })
            .unwrap();
        assert_eq!(search.hits.len(), 1);
        assert!(search.retrieval_info.vector_active);
    }

    #[test]
    fn incremental_publication_reuses_unchanged_vectors_and_activates_staged_document() {
        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);
        publish_all(&home, &store, Some(&embedder)).unwrap();
        let staged = crate::ops::document_ingestion::stage_document_bytes(
            &store,
            &PlainTextParser,
            "changed.txt",
            b"new incrementally embedded evidence",
        )
        .unwrap();
        assert_eq!(
            store.get_document(&staged.doc_id).unwrap().1.as_str(),
            "INDEXING"
        );

        let output = publish_incremental_staged(
            &home,
            &store,
            &embedder,
            std::slice::from_ref(&staged.doc_id),
            1,
        )
        .unwrap();
        assert_eq!(output.embedded_segments, 1);
        assert_eq!(output.reused_segments, 1);
        assert_eq!(output.embedding_batches, 1);
        assert_eq!(
            store.get_document(&staged.doc_id).unwrap().1.as_str(),
            "READY"
        );
        verify_active_manifest_artifacts(&home, &store).unwrap();
    }

    #[test]
    fn startup_rejects_tampered_manifest_artifacts() {
        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);
        publish_all(&home, &store, Some(&embedder)).unwrap();
        let manifest = store.active_corpus_manifest().unwrap().unwrap();
        let vector_directory =
            home.vector_generation_dir(manifest.vector_generation.unwrap().as_u64());
        std::fs::write(
            vector_directory.join("unexpected-data").as_std_path(),
            b"tamper",
        )
        .unwrap();

        let error = match crate::Engine::open(home) {
            Ok(_) => panic!("tampered active artifact must not open"),
            Err(error) => error,
        };
        assert!(matches!(error, ShiroError::IndexBuildVec { .. }));
    }

    #[test]
    fn every_pre_activation_failure_preserves_previous_complete_manifest() {
        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);
        publish_all(&home, &store, Some(&embedder)).unwrap();
        let previous = store.active_corpus_manifest().unwrap().unwrap();
        let failure_points = [
            PublicationCheckpoint::FtsGenerationReserved,
            PublicationCheckpoint::FtsArtifactValidated,
            PublicationCheckpoint::VectorGenerationReserved,
            PublicationCheckpoint::VectorArtifactValidated,
            PublicationCheckpoint::BeforeManifestActivation,
        ];

        for failure_point in failure_points {
            let result =
                publish_all_with_checkpoint(&home, &store, Some(&embedder), |checkpoint| {
                    if checkpoint == failure_point {
                        Err(ShiroError::IndexBuildFts {
                            message: format!("injected failure at {checkpoint:?}"),
                        })
                    } else {
                        Ok(())
                    }
                });
            assert!(result.is_err());
            assert_eq!(
                store.active_corpus_manifest().unwrap(),
                Some(previous.clone())
            );
            verify_active_manifest_artifacts(&home, &store).unwrap();
        }
    }

    #[test]
    fn orphan_cleanup_removes_only_generations_absent_from_every_manifest() {
        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);
        publish_all(&home, &store, Some(&embedder)).unwrap();
        let active = store.active_corpus_manifest().unwrap().unwrap();
        let orphaned_fts = store
            .reserve_generation("fts", 0, 0, "2025-01-01T00:00:00Z")
            .unwrap();
        let orphaned_vector = store
            .reserve_generation("vector", 0, 0, "2025-01-01T00:00:00Z")
            .unwrap();
        std::fs::create_dir_all(
            home.tantivy_generation_dir(orphaned_fts.as_u64())
                .as_std_path(),
        )
        .unwrap();
        std::fs::create_dir_all(
            home.vector_generation_dir(orphaned_vector.as_u64())
                .as_std_path(),
        )
        .unwrap();
        let noncanonical_name = home.root().join("tantivy_gen_01");
        std::fs::create_dir_all(noncanonical_name.as_std_path()).unwrap();

        let cleanup = garbage_collect_orphaned_generation_artifacts(&home, &store).unwrap();

        assert_eq!(cleanup.removed_fts_generations, vec![orphaned_fts]);
        assert_eq!(cleanup.removed_vector_generations, vec![orphaned_vector]);
        assert!(home
            .tantivy_generation_dir(active.fts_generation.as_u64())
            .as_std_path()
            .is_dir());
        assert!(home
            .vector_generation_dir(active.vector_generation.unwrap().as_u64())
            .as_std_path()
            .is_dir());
        assert!(noncanonical_name.as_std_path().is_dir());
        assert_eq!(
            store
                .reserve_generation("fts", 0, 0, "2025-01-01T00:00:01Z")
                .unwrap(),
            orphaned_fts.next()
        );
        verify_active_manifest_artifacts(&home, &store).unwrap();
    }

    #[test]
    fn process_termination_around_manifest_activation_preserves_a_complete_corpus() {
        if run_publication_crash_child_if_requested() {
            return;
        }

        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);
        publish_all(&home, &store, Some(&embedder)).unwrap();
        let original = store.active_corpus_manifest().unwrap().unwrap();

        terminate_publication_child_at_checkpoint(
            &home,
            PublicationCheckpoint::BeforeManifestActivation,
        );
        let before_activation = store.active_corpus_manifest().unwrap().unwrap();
        assert_eq!(before_activation, original);
        assert_active_manifest_is_complete(&home, &store, &before_activation);

        terminate_publication_child_at_checkpoint(
            &home,
            PublicationCheckpoint::AfterManifestActivation,
        );
        let after_activation = store.active_corpus_manifest().unwrap().unwrap();
        assert_ne!(after_activation.manifest_id, original.manifest_id);
        assert_active_manifest_is_complete(&home, &store, &after_activation);
    }

    fn run_publication_crash_child_if_requested() -> bool {
        let Ok(root) = std::env::var("SHIRO_TEST_PUBLICATION_CRASH_HOME") else {
            return false;
        };
        let checkpoint_name = std::env::var("SHIRO_TEST_PUBLICATION_CRASH_CHECKPOINT").unwrap();
        let marker = std::env::var("SHIRO_TEST_PUBLICATION_CRASH_MARKER").unwrap();
        let target = match checkpoint_name.as_str() {
            "before_manifest_activation" => PublicationCheckpoint::BeforeManifestActivation,
            "after_manifest_activation" => PublicationCheckpoint::AfterManifestActivation,
            other => panic!("unknown publication crash checkpoint: {other}"),
        };
        let home = ShiroHome::new(camino::Utf8PathBuf::from(root));
        let store = Store::open(&home.db_path()).unwrap();
        let embedder = DeterministicStubEmbedder::new(8);
        let result = publish_all_with_checkpoint(&home, &store, Some(&embedder), |checkpoint| {
            if checkpoint == target {
                std::fs::write(&marker, checkpoint_name.as_bytes()).unwrap();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            }
            Ok(())
        });
        panic!("publication crash child reached an unexpected result: {result:?}");
    }

    fn terminate_publication_child_at_checkpoint(
        home: &ShiroHome,
        checkpoint: PublicationCheckpoint,
    ) {
        let checkpoint_name = match checkpoint {
            PublicationCheckpoint::BeforeManifestActivation => "before_manifest_activation",
            PublicationCheckpoint::AfterManifestActivation => "after_manifest_activation",
            other => panic!("unsupported process crash checkpoint: {other:?}"),
        };
        let marker = home
            .root()
            .join(format!("publication-crash-{checkpoint_name}.marker"));
        let current_test_binary = std::env::current_exe().unwrap();
        let mut child = std::process::Command::new(current_test_binary)
            .arg("--exact")
            .arg(
                "ops::corpus_publication::tests::process_termination_around_manifest_activation_preserves_a_complete_corpus",
            )
            .arg("--nocapture")
            .env("SHIRO_TEST_PUBLICATION_CRASH_HOME", home.root().as_str())
            .env(
                "SHIRO_TEST_PUBLICATION_CRASH_CHECKPOINT",
                checkpoint_name,
            )
            .env("SHIRO_TEST_PUBLICATION_CRASH_MARKER", marker.as_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if marker.as_std_path().is_file() {
                break;
            }
            if let Some(status) = child.try_wait().unwrap() {
                panic!("publication crash child exited before checkpoint: {status}");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "publication crash child did not reach {checkpoint_name}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    fn assert_active_manifest_is_complete(
        home: &ShiroHome,
        store: &Store,
        manifest: &CorpusManifest,
    ) {
        assert_eq!(
            store.active_generation("fts").unwrap(),
            manifest.fts_generation
        );
        assert_eq!(
            store.active_generation("vector").unwrap(),
            manifest.vector_generation.unwrap()
        );
        verify_active_manifest_artifacts(home, store).unwrap();
        crate::Engine::open(home.clone()).unwrap();
    }

    #[test]
    fn failure_after_activation_leaves_new_complete_manifest_readable() {
        let (_temporary, home, store) = populated_home();
        let embedder = DeterministicStubEmbedder::new(8);
        publish_all(&home, &store, Some(&embedder)).unwrap();
        let previous = store.active_corpus_manifest().unwrap().unwrap();

        let result = publish_all_with_checkpoint(&home, &store, Some(&embedder), |checkpoint| {
            if checkpoint == PublicationCheckpoint::AfterManifestActivation {
                Err(ShiroError::IndexBuildFts {
                    message: "injected post-activation failure".to_string(),
                })
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        let active = store.active_corpus_manifest().unwrap().unwrap();
        assert_ne!(active.manifest_id, previous.manifest_id);
        verify_active_manifest_artifacts(&home, &store).unwrap();
        assert_eq!(
            artifact_digest(
                &home.vector_generation_dir(active.vector_generation.unwrap().as_u64())
            )
            .unwrap(),
            active.vector_digest.unwrap()
        );
    }
}
