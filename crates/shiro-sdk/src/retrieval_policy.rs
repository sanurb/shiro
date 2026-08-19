//! Internal retrieval routing, depth, and mandatory eligibility policy.
//!
//! This module is the single policy home for deciding which retrieval stages
//! run and how deeply each stage may retrieve. Authoritative document scope is
//! resolved here before source-specific retrieval.

use std::collections::HashSet;

use shiro_core::ports::Reranker;
use shiro_core::{ConceptId, DocId, SegmentId, ShiroError};
use shiro_store::Store;

use crate::ops::search::{SearchFilters, SearchInput, SearchMode};

/// Validated filter values used to resolve the authoritative candidate universe.
pub(crate) struct ResolvedSearchFilters {
    tags: HashSet<String>,
    concept_ids: HashSet<ConceptId>,
    document_ids: HashSet<DocId>,
}

impl ResolvedSearchFilters {
    pub(crate) fn resolve(store: &Store, filters: &SearchFilters) -> Result<Self, ShiroError> {
        let tags = filters
            .tags
            .iter()
            .map(|tag| tag.trim().to_lowercase())
            .filter(|tag| !tag.is_empty())
            .collect();
        let requested_concept_ids = filters
            .concept_ids
            .iter()
            .map(|concept_id| {
                ConceptId::from_stored(concept_id.clone()).map_err(|error| {
                    ShiroError::InvalidInput {
                        message: format!("invalid concept filter '{concept_id}': {error}"),
                    }
                })
            })
            .collect::<Result<HashSet<_>, _>>()?;
        let mut concept_ids = requested_concept_ids.clone();
        for ancestor_id in requested_concept_ids {
            concept_ids.extend(store.get_concept_descendant_ids(&ancestor_id)?);
        }
        let document_ids = filters
            .document_ids
            .iter()
            .map(|document_id| {
                DocId::from_stored(document_id.clone()).map_err(|error| ShiroError::InvalidInput {
                    message: format!("invalid document filter '{document_id}': {error}"),
                })
            })
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(Self {
            tags,
            concept_ids,
            document_ids,
        })
    }

    pub(crate) fn matches_document(
        &self,
        store: &Store,
        doc_id: &DocId,
    ) -> Result<bool, ShiroError> {
        if !self.document_ids.is_empty() && !self.document_ids.contains(doc_id) {
            return Ok(false);
        }
        if !self.tags.is_empty() {
            let Some(enrichment) = store.get_enrichment(doc_id)? else {
                return Ok(false);
            };
            if !enrichment
                .tags
                .iter()
                .map(|tag| tag.to_lowercase())
                .any(|tag| self.tags.contains(&tag))
            {
                return Ok(false);
            }
        }
        if !self.concept_ids.is_empty() {
            let assigned = store.get_doc_concepts(doc_id)?;
            if !assigned
                .iter()
                .any(|(concept_id, _, _)| self.concept_ids.contains(concept_id))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Authoritative candidate universe resolved from READY documents in SQLite.
pub(crate) struct RetrievalScope {
    eligible_document_ids: Vec<DocId>,
    eligible_document_id_set: HashSet<DocId>,
    eligible_segment_ids: HashSet<SegmentId>,
}

impl RetrievalScope {
    fn resolve(store: &Store, filters: &ResolvedSearchFilters) -> Result<Self, ShiroError> {
        let eligible_pairs = store.ready_document_segment_ids()?;
        let mut eligible_document_id_set = HashSet::new();
        let mut eligible_segment_ids = HashSet::new();
        let mut document_matches = std::collections::HashMap::new();
        for (doc_id, segment_id) in eligible_pairs {
            let matches = match document_matches.get(&doc_id) {
                Some(matches) => *matches,
                None => {
                    let matches = filters.matches_document(store, &doc_id)?;
                    document_matches.insert(doc_id.clone(), matches);
                    matches
                }
            };
            if matches {
                eligible_document_id_set.insert(doc_id);
                eligible_segment_ids.insert(segment_id);
            }
        }
        let mut eligible_document_ids: Vec<DocId> =
            eligible_document_id_set.iter().cloned().collect();
        eligible_document_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(Self {
            eligible_document_ids,
            eligible_document_id_set,
            eligible_segment_ids,
        })
    }

    /// Re-resolve the exact saved filters before exposing cached explain evidence.
    pub(crate) fn resolve_saved_policy(
        store: &Store,
        retrieval_policy_json: &str,
    ) -> Result<Self, ShiroError> {
        let policy: serde_json::Value =
            serde_json::from_str(retrieval_policy_json).map_err(|error| {
                ShiroError::StoreCorrupt {
                    message: format!("invalid saved retrieval policy: {error}"),
                }
            })?;
        let filters = policy
            .get("filters")
            .cloned()
            .map(serde_json::from_value::<SearchFilters>)
            .transpose()
            .map_err(|error| ShiroError::StoreCorrupt {
                message: format!("invalid saved retrieval filters: {error}"),
            })?
            .unwrap_or_default();
        let filters = ResolvedSearchFilters::resolve(store, &filters)?;
        Self::resolve(store, &filters)
    }

    /// Document IDs that retrieval sources may consider.
    pub(crate) fn eligible_document_ids(&self) -> &[DocId] {
        &self.eligible_document_ids
    }

    /// Stable digest of the exact authoritative candidate universe.
    fn digest(&self) -> String {
        let mut eligible_segment_ids: Vec<&str> = self
            .eligible_segment_ids
            .iter()
            .map(SegmentId::as_str)
            .collect();
        eligible_segment_ids.sort_unstable();

        let mut hasher = blake3::Hasher::new();
        for doc_id in &self.eligible_document_ids {
            hasher.update(b"doc\0");
            hasher.update(doc_id.as_str().as_bytes());
            hasher.update(b"\0");
        }
        for segment_id in eligible_segment_ids {
            hasher.update(b"segment\0");
            hasher.update(segment_id.as_bytes());
            hasher.update(b"\0");
        }
        hasher.finalize().to_hex().to_string()
    }

    /// Check whether a segment belongs to the READY candidate universe.
    pub(crate) fn contains_segment(&self, segment_id: &SegmentId) -> bool {
        self.eligible_segment_ids.contains(segment_id)
    }

    /// Check that both indexed identities belong to the READY candidate universe.
    pub(crate) fn contains(&self, doc_id: &DocId, segment_id: &SegmentId) -> bool {
        self.eligible_document_id_set.contains(doc_id)
            && self.eligible_segment_ids.contains(segment_id)
    }
}

/// Fully resolved internal retrieval route and independent depth limits.
pub(crate) struct ResolvedRetrievalPolicy {
    pub(crate) use_bm25: bool,
    pub(crate) use_vector: bool,
    pub(crate) use_reranker: bool,
    pub(crate) rerank_candidate_limit: usize,
    pub(crate) source_candidate_limit: usize,
    pub(crate) scope: RetrievalScope,
}

impl ResolvedRetrievalPolicy {
    /// Resolve active retrieval stages without changing the public search contract.
    pub(crate) fn resolve(
        store: &Store,
        input: &SearchInput,
        vector_pair_available: bool,
        reranker: Option<&dyn Reranker>,
    ) -> Result<Self, ShiroError> {
        let vector_pair_required = !matches!(input.mode, SearchMode::Bm25);
        if vector_pair_required
            && matches!(input.mode, SearchMode::Vector)
            && !vector_pair_available
        {
            return Err(ShiroError::SearchFailed {
                message:
                    "Vector search requested but no compatible embedder and vector index are configured"
                        .to_string(),
            });
        }

        let use_bm25 = !matches!(input.mode, SearchMode::Vector);
        let use_vector = vector_pair_required && vector_pair_available;
        let use_reranker = input.rerank && reranker.is_some();
        let rerank_candidate_limit = if use_reranker && input.limit > 0 {
            reranker
                .map(|active_reranker| active_reranker.rerank_candidate_limit().candidate_count())
                .unwrap_or(0)
        } else {
            0
        };

        let filters = ResolvedSearchFilters::resolve(store, &input.filters)?;
        Ok(Self {
            use_bm25,
            use_vector,
            use_reranker,
            rerank_candidate_limit,
            source_candidate_limit: input.limit.max(rerank_candidate_limit),
            scope: RetrievalScope::resolve(store, &filters)?,
        })
    }

    /// Serialize every retrieval choice that can affect the returned evidence.
    pub(crate) fn snapshot_json(
        &self,
        input: &SearchInput,
        embedding_fingerprint: Option<&str>,
        reranker_model: Option<&str>,
    ) -> String {
        serde_json::json!({
            "schema_version": 1,
            "mode": input.mode.as_str(),
            "sources": {
                "bm25": self.use_bm25,
                "vector": self.use_vector,
                "embedding_fingerprint": embedding_fingerprint,
            },
            "eligibility": {
                "required_document_state": "READY",
                "scope_digest": self.scope.digest(),
            },
            "depth": {
                "source_candidates": self.source_candidate_limit,
                "rerank_candidates": self.rerank_candidate_limit,
                "result_limit": input.limit,
            },
            "reranker": {
                "requested": input.rerank,
                "active": reranker_model.is_some(),
                "model": reranker_model,
            },
            "retrieval_text": {
                "version": shiro_parse::RETRIEVAL_TEXT_VERSION,
                "max_bytes": shiro_parse::MAX_RETRIEVAL_TEXT_BYTES,
                "includes_document_title": true,
                "includes_heading_path": true,
            },
            "context": {
                "expand": input.expand,
                "max_blocks": input.max_blocks,
                "max_chars": input.max_chars,
            },
            "filters": {
                "tags": input.filters.tags,
                "concept_ids": input.filters.concept_ids,
                "document_ids": input.filters.document_ids,
                "semantics": "or_within_fields_and_across_fields",
            },
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::{Concept, ConceptRelation, SkosRelation};

    fn test_concept(label: &str) -> Concept {
        Concept {
            id: ConceptId::new("urn:test:retrieval-policy", label),
            scheme_uri: "urn:test:retrieval-policy".to_string(),
            pref_label: label.to_string(),
            alt_labels: Vec::new(),
            definition: None,
        }
    }

    #[test]
    fn resolved_concept_filters_include_transitive_descendants() {
        let directory = tempfile::TempDir::new().unwrap();
        let database_path = camino::Utf8Path::from_path(directory.path())
            .unwrap()
            .join("shiro.db");
        let store = Store::open(&database_path).unwrap();
        let ancestor = test_concept("Ancestor");
        let child = test_concept("Child");
        let grandchild = test_concept("Grandchild");
        for concept in [&ancestor, &child, &grandchild] {
            store.put_concept(concept).unwrap();
        }
        for relation in [
            ConceptRelation {
                from: child.id.clone(),
                to: ancestor.id.clone(),
                relation: SkosRelation::Broader,
            },
            ConceptRelation {
                from: grandchild.id.clone(),
                to: child.id.clone(),
                relation: SkosRelation::Broader,
            },
        ] {
            store.put_concept_relation(&relation).unwrap();
        }
        store.rebuild_closure().unwrap();

        let resolved = ResolvedSearchFilters::resolve(
            &store,
            &SearchFilters {
                concept_ids: vec![ancestor.id.as_str().to_string()],
                ..SearchFilters::default()
            },
        )
        .unwrap();

        assert!(resolved.concept_ids.contains(&ancestor.id));
        assert!(resolved.concept_ids.contains(&child.id));
        assert!(resolved.concept_ids.contains(&grandchild.id));
    }
}
