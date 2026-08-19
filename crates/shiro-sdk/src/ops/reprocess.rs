//! Scoped, bounded reprocessing planner and executor.

use serde::{Deserialize, Serialize};
use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::ports::{Embedder, Parser};
use shiro_core::{DocId, DocState, ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_parse::SEGMENTER_VERSION;
use shiro_store::Store;

use super::corpus_publication::{publish_all, verify_active_manifest_artifacts};
use super::document_ingestion::{
    publish_staged_documents, stage_document_bytes_with_force, StagedDocumentIngestion,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReprocessTarget {
    Parse,
    Derived,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReprocessLimits {
    pub max_documents: usize,
    pub max_source_bytes: usize,
    pub max_model_calls: usize,
    pub embedding_batch_size: usize,
}

impl Default for ReprocessLimits {
    fn default() -> Self {
        Self {
            max_documents: 100,
            max_source_bytes: 512 * 1024 * 1024,
            max_model_calls: 100_000,
            embedding_batch_size: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReprocessInput {
    #[serde(default)]
    pub document_ids: Vec<String>,
    pub target: ReprocessTarget,
    #[serde(default)]
    pub execute: bool,
    #[serde(default)]
    pub include_vector: bool,
    #[serde(default)]
    pub resume_manifest_id: Option<String>,
    #[serde(default)]
    pub limits: ReprocessLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReprocessDocumentPlan {
    pub doc_id: String,
    pub source_uri: String,
    pub source_bytes: usize,
    pub current_segments: usize,
    pub processing_fingerprint_stale: bool,
    pub selected_stages: Vec<String>,
    pub transitive_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReprocessPlan {
    pub documents: Vec<ReprocessDocumentPlan>,
    pub estimated_source_bytes: usize,
    pub estimated_model_calls: usize,
    pub estimated_embedding_batches: usize,
    pub execution_allowed: bool,
    pub blockers: Vec<String>,
    pub rollback_manifest_id: Option<String>,
    pub rollback_fts_generation: u64,
    pub rollback_vector_generation: Option<u64>,
    pub limits: ReprocessLimits,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReprocessOutput {
    pub status: String,
    pub plan: ReprocessPlan,
    pub publication: Option<serde_json::Value>,
}

pub fn plan(
    store: &Store,
    parser: &dyn Parser,
    embedder: Option<&dyn Embedder>,
    input: &ReprocessInput,
) -> Result<ReprocessPlan, ShiroError> {
    let requested_ids = resolve_document_ids(store, &input.document_ids)?;
    let requested_fingerprint =
        ProcessingFingerprint::new(parser.name(), parser.version(), SEGMENTER_VERSION);
    let active = store.active_corpus_manifest()?;
    let mut documents = Vec::with_capacity(requested_ids.len());
    let mut estimated_source_bytes = 0usize;
    let mut selected_segment_count = 0usize;

    for doc_id in requested_ids {
        let (document, state) = store.get_document(&doc_id)?;
        if state != DocState::Ready {
            return Err(ShiroError::InvalidInput {
                message: format!("reprocess requires READY document: {doc_id} is {state:?}"),
            });
        }
        let source = store.get_blob(&document.metadata.source_hash)?;
        let current_segments = store.get_segments(&doc_id)?.len();
        let stale = store
            .get_fingerprint(&doc_id)?
            .map(|stored| stored.content_hash() != requested_fingerprint.content_hash())
            .unwrap_or(true);
        let mut selected_stages = Vec::new();
        let mut transitive_artifacts = Vec::new();
        if matches!(input.target, ReprocessTarget::Parse | ReprocessTarget::All) {
            selected_stages.extend([
                "parse".to_string(),
                "segment".to_string(),
                "retrieval_text".to_string(),
            ]);
            transitive_artifacts.push("canonical_graph".to_string());
        }
        selected_stages.push("fts".to_string());
        transitive_artifacts.push("fts_generation".to_string());
        if input.include_vector {
            selected_stages.push("vector".to_string());
            transitive_artifacts.push("vector_generation".to_string());
        }
        estimated_source_bytes = estimated_source_bytes.saturating_add(source.len());
        selected_segment_count = selected_segment_count.saturating_add(current_segments);
        documents.push(ReprocessDocumentPlan {
            doc_id: doc_id.as_str().to_string(),
            source_uri: document.metadata.source_uri,
            source_bytes: source.len(),
            current_segments,
            processing_fingerprint_stale: stale,
            selected_stages,
            transitive_artifacts,
        });
    }

    let estimated_model_calls = if input.include_vector {
        selected_segment_count
    } else {
        0
    };
    let estimated_embedding_batches = if estimated_model_calls == 0 {
        0
    } else {
        estimated_model_calls.div_ceil(input.limits.embedding_batch_size.max(1))
    };
    let mut blockers = Vec::new();
    if documents.len() > input.limits.max_documents {
        blockers.push(format!(
            "selected {} documents exceeds max_documents {}",
            documents.len(),
            input.limits.max_documents
        ));
    }
    if estimated_source_bytes > input.limits.max_source_bytes {
        blockers.push(format!(
            "estimated {estimated_source_bytes} source bytes exceeds max_source_bytes {}",
            input.limits.max_source_bytes
        ));
    }
    if estimated_model_calls > input.limits.max_model_calls {
        blockers.push(format!(
            "estimated {estimated_model_calls} model calls exceeds max_model_calls {}",
            input.limits.max_model_calls
        ));
    }
    if input.limits.embedding_batch_size == 0 {
        blockers.push("embedding_batch_size must be positive".to_string());
    }
    if input.include_vector && embedder.is_none() {
        blockers.push("vector reprocessing requires a configured embedder".to_string());
    }
    if let Some(expected) = &input.resume_manifest_id {
        if active.as_ref().map(|manifest| &manifest.manifest_id) != Some(expected) {
            blockers.push(format!(
                "resume manifest {expected} is not the active verified rollback point"
            ));
        }
    }

    Ok(ReprocessPlan {
        documents,
        estimated_source_bytes,
        estimated_model_calls,
        estimated_embedding_batches,
        execution_allowed: blockers.is_empty(),
        blockers,
        rollback_manifest_id: active.as_ref().map(|manifest| manifest.manifest_id.clone()),
        rollback_fts_generation: active
            .as_ref()
            .map(|manifest| manifest.fts_generation.as_u64())
            .unwrap_or(0),
        rollback_vector_generation: active
            .as_ref()
            .and_then(|manifest| manifest.vector_generation)
            .map(|generation| generation.as_u64()),
        limits: input.limits.clone(),
    })
}

pub fn execute(
    home: &ShiroHome,
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    embedder: Option<&dyn Embedder>,
    input: &ReprocessInput,
) -> Result<ReprocessOutput, ShiroError> {
    let plan = plan(store, parser, embedder, input)?;
    if !input.execute {
        return Ok(ReprocessOutput {
            status: "planned".to_string(),
            plan,
            publication: None,
        });
    }
    if !plan.execution_allowed {
        return Err(ShiroError::InvalidInput {
            message: format!("reprocess plan is blocked: {}", plan.blockers.join("; ")),
        });
    }
    verify_active_manifest_artifacts(home, store)?;

    let publication = if input.include_vector {
        let embedder = embedder.ok_or_else(|| ShiroError::InvalidInput {
            message: "vector reprocessing requires a configured embedder".to_string(),
        })?;
        let generations = super::corpus_publication::reserve_incremental_generations(store)?;
        let incremental = store.with_atomic_corpus_publication(|| {
            let staged = stage_reprocess_documents(store, parser, input, &plan)?;
            let staged_ids = staged
                .iter()
                .map(|document| document.doc_id.clone())
                .collect::<Vec<_>>();
            super::corpus_publication::publish_reserved_incremental_staged(
                home,
                store,
                embedder,
                &staged_ids,
                input.limits.embedding_batch_size,
                generations,
            )
        })?;
        serde_json::to_value(incremental).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize incremental publication: {error}"),
        })?
    } else {
        let staged = stage_reprocess_documents(store, parser, input, &plan)?;
        if !staged.is_empty() {
            let staged_refs = staged.iter().collect::<Vec<_>>();
            publish_staged_documents(store, fts, &staged_refs)?;
        }
        serde_json::to_value(publish_all(home, store, None)?).map_err(|error| {
            ShiroError::StoreCorrupt {
                message: format!("failed to serialize reprocess publication: {error}"),
            }
        })?
    };

    Ok(ReprocessOutput {
        status: "executed".to_string(),
        plan,
        publication: Some(publication),
    })
}

fn stage_reprocess_documents(
    store: &Store,
    parser: &dyn Parser,
    input: &ReprocessInput,
    plan: &ReprocessPlan,
) -> Result<Vec<StagedDocumentIngestion>, ShiroError> {
    if !matches!(input.target, ReprocessTarget::Parse | ReprocessTarget::All) {
        return Ok(Vec::new());
    }
    let mut staged = Vec::with_capacity(plan.documents.len());
    for document_plan in &plan.documents {
        let doc_id = DocId::from_stored(&document_plan.doc_id).map_err(|message| {
            ShiroError::InvalidInput {
                message: message.to_string(),
            }
        })?;
        let (document, _) = store.get_document(&doc_id)?;
        let source = store.get_blob(&document.metadata.source_hash)?;
        match stage_document_bytes_with_force(
            store,
            parser,
            &document.metadata.source_uri,
            &source,
            true,
        ) {
            Ok(document) => staged.push(document),
            Err(error) => {
                for staged_document in &staged {
                    if let Err(state_error) =
                        store.set_state(&staged_document.doc_id, DocState::Failed)
                    {
                        tracing::error!(
                            %state_error,
                            doc_id = %staged_document.doc_id,
                            "failed to mark reprocessing stage failed"
                        );
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(staged)
}

fn resolve_document_ids(store: &Store, values: &[String]) -> Result<Vec<DocId>, ShiroError> {
    if values.is_empty() {
        let mut documents = store
            .list_all_documents()?
            .into_iter()
            .filter(|(_, state, _)| *state == DocState::Ready)
            .map(|(doc_id, _, _)| doc_id)
            .collect::<Vec<_>>();
        documents.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        return Ok(documents);
    }
    let mut seen = std::collections::HashSet::new();
    let mut documents = Vec::new();
    for value in values {
        let doc_id = DocId::from_stored(value).map_err(|message| ShiroError::InvalidInput {
            message: message.to_string(),
        })?;
        if seen.insert(doc_id.clone()) {
            documents.push(doc_id);
        }
    }
    documents.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(documents)
}
