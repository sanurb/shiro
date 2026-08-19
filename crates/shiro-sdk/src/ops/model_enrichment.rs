//! Attributed, reversible model-enrichment proposals.

use serde::{Deserialize, Serialize};
use shiro_core::ports::Embedder;
use shiro_core::taxonomy::{Concept, ConceptId};
use shiro_core::{DocId, DocState, ShiroError};
use shiro_store::{ModelEnrichmentProposalRecord, ProposedConceptAssignment, Store};

use super::read::resolve_doc_id;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposedModelConcept {
    pub scheme_uri: String,
    pub pref_label: String,
    #[serde(default)]
    pub alt_labels: Vec<String>,
    #[serde(default)]
    pub definition: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelEnrichmentProposalInput {
    pub doc_id: String,
    pub provider: String,
    pub model: String,
    pub actor_id: String,
    pub data_region: String,
    pub retention_policy: String,
    pub consent_id: String,
    pub concepts: Vec<ProposedModelConcept>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelEnrichmentProposalOutput {
    pub proposal_id: String,
    pub doc_id: String,
    pub status: String,
    pub trust_zone: String,
    pub proposed_concept_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelEnrichmentResolutionAction {
    Promote,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelEnrichmentResolutionInput {
    pub proposal_id: String,
    pub action: ModelEnrichmentResolutionAction,
    pub resolved_actor_id: String,
    pub approval_id: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelEnrichmentResolutionOutput {
    pub proposal_id: String,
    pub status: String,
    pub applied_concept_ids: Vec<String>,
}

pub fn propose(
    store: &Store,
    input: &ModelEnrichmentProposalInput,
) -> Result<ModelEnrichmentProposalOutput, ShiroError> {
    validate_proposal(input)?;
    let doc_id = resolve_doc_id(store, &input.doc_id)?;
    let (_, state) = store.get_document(&doc_id)?;
    if state != DocState::Ready {
        return Err(ShiroError::InvalidInput {
            message: format!("model enrichment requires a READY document: {doc_id}"),
        });
    }
    let concepts = input
        .concepts
        .iter()
        .map(|concept| ConceptId::new(&concept.scheme_uri, &concept.pref_label))
        .collect::<Vec<_>>();
    let payload_json =
        serde_json::to_string(&input.concepts).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize model proposal: {error}"),
        })?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(doc_id.as_str().as_bytes());
    hasher.update(payload_json.as_bytes());
    hasher.update(shiro_core::RunId::generate().as_str().as_bytes());
    let proposal_id = format!("proposal_{}", hasher.finalize().to_hex());
    store.put_model_enrichment_proposal(&ModelEnrichmentProposalRecord {
        proposal_id: proposal_id.clone(),
        doc_id: doc_id.clone(),
        provider: input.provider.clone(),
        model: input.model.clone(),
        actor_id: input.actor_id.clone(),
        data_region: input.data_region.clone(),
        retention_policy: input.retention_policy.clone(),
        consent_id: input.consent_id.clone(),
        payload_json,
        status: "PROPOSED".to_string(),
        applied_concepts_json: "[]".to_string(),
    })?;
    Ok(ModelEnrichmentProposalOutput {
        proposal_id,
        doc_id: doc_id.as_str().to_string(),
        status: "PROPOSED".to_string(),
        trust_zone: "PROPOSED".to_string(),
        proposed_concept_ids: concepts
            .into_iter()
            .map(|concept| concept.as_str().to_string())
            .collect(),
    })
}

pub fn resolve(
    store: &Store,
    input: &ModelEnrichmentResolutionInput,
) -> Result<ModelEnrichmentResolutionOutput, ShiroError> {
    if input.resolved_actor_id.trim().is_empty() || input.approval_id.trim().is_empty() {
        return Err(ShiroError::InvalidInput {
            message: "model enrichment resolution requires actor and approval IDs".to_string(),
        });
    }
    let proposal = store.get_model_enrichment_proposal(&input.proposal_id)?;
    match input.action {
        ModelEnrichmentResolutionAction::Promote => {
            let concepts: Vec<ProposedModelConcept> = serde_json::from_str(&proposal.payload_json)
                .map_err(|error| ShiroError::StoreCorrupt {
                    message: format!("invalid model enrichment proposal payload: {error}"),
                })?;
            let assignments = concepts
                .into_iter()
                .map(|concept| ProposedConceptAssignment {
                    concept: Concept {
                        id: ConceptId::new(&concept.scheme_uri, &concept.pref_label),
                        scheme_uri: concept.scheme_uri,
                        pref_label: concept.pref_label,
                        alt_labels: concept.alt_labels,
                        definition: concept.definition,
                    },
                    confidence: concept.confidence,
                })
                .collect::<Vec<_>>();
            let applied = store.promote_model_enrichment_proposal(
                &input.proposal_id,
                &assignments,
                &input.resolved_actor_id,
                &input.approval_id,
            )?;
            Ok(ModelEnrichmentResolutionOutput {
                proposal_id: input.proposal_id.clone(),
                status: "PROMOTED".to_string(),
                applied_concept_ids: applied
                    .into_iter()
                    .map(|concept| concept.as_str().to_string())
                    .collect(),
            })
        }
        ModelEnrichmentResolutionAction::Reject => {
            store.reject_model_enrichment_proposal(
                &input.proposal_id,
                &input.resolved_actor_id,
                &input.approval_id,
            )?;
            Ok(ModelEnrichmentResolutionOutput {
                proposal_id: input.proposal_id.clone(),
                status: "REJECTED".to_string(),
                applied_concept_ids: Vec::new(),
            })
        }
    }
}

/// Suggest assignments to existing concepts and persist one isolated PROPOSED record.
pub(crate) fn propose_automatic_document_concepts(
    store: &Store,
    embedder: Option<&dyn Embedder>,
    doc_id: &DocId,
) -> Result<Option<ModelEnrichmentProposalOutput>, ShiroError> {
    let concepts = store.list_concepts(10_000)?;
    if concepts.is_empty() {
        return Ok(None);
    }
    let (document, state) = store.get_document(doc_id)?;
    if state != DocState::Ready {
        return Err(ShiroError::InvalidInput {
            message: format!("automatic concept proposal requires a READY document: {doc_id}"),
        });
    }
    let text = document
        .rendered_text
        .as_deref()
        .unwrap_or(&document.canonical_text);
    let title = super::enrich::extract_title(text);
    let tags = super::enrich::extract_tags(text);
    let summary = super::enrich::build_summary(text);
    let tags_text = tags.join(" ");
    let suggestion_text = [
        title.as_deref().unwrap_or_default(),
        summary.as_deref().unwrap_or_default(),
        &tags_text,
    ]
    .join("\n");
    let document_embedding = embedder
        .map(|active_embedder| active_embedder.embed(&suggestion_text))
        .transpose()?;

    let mut suggestions = Vec::new();
    for concept in concepts {
        let lexical_confidence = heuristic_concept_confidence(text, &tags, &concept);
        let semantic_confidence = match (embedder, document_embedding.as_deref()) {
            (Some(active_embedder), Some(document_vector)) => {
                let concept_text = concept_suggestion_text(&concept);
                let concept_vector = active_embedder.embed(&concept_text)?;
                cosine_similarity(document_vector, &concept_vector)
                    .filter(|score| *score >= 0.80)
                    .map(|score| 0.80 + ((score - 0.80) * 0.75))
                    .unwrap_or(0.0)
            }
            _ => 0.0,
        };
        let confidence = lexical_confidence.max(semantic_confidence);
        if confidence >= 0.70 {
            suggestions.push((concept, confidence.min(1.0)));
        }
    }
    suggestions.sort_by(|(left_concept, left_score), (right_concept, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left_concept.id.as_str().cmp(right_concept.id.as_str()))
    });
    suggestions.truncate(5);
    if suggestions.is_empty() {
        return Ok(None);
    }

    let embedding_meta = embedder.map(|active_embedder| active_embedder.meta());
    let provider = embedding_meta
        .as_ref()
        .map(|meta| format!("shiro-auto-tagger/{}", meta.provider))
        .unwrap_or_else(|| "shiro-auto-tagger/heuristic".to_string());
    let model = embedding_meta
        .as_ref()
        .map(|meta| format!("heuristic-v1+{}", meta.model_name))
        .unwrap_or_else(|| "heuristic-v1".to_string());
    let external_provider = embedding_meta
        .as_ref()
        .is_some_and(|meta| meta.provider != "fastembed");
    let input = ModelEnrichmentProposalInput {
        doc_id: doc_id.as_str().to_string(),
        provider,
        model,
        actor_id: "shiro:auto-tagger".to_string(),
        data_region: if external_provider {
            "configured-provider-unspecified".to_string()
        } else {
            "local".to_string()
        },
        retention_policy: if external_provider {
            "configured-provider-unspecified".to_string()
        } else {
            "none".to_string()
        },
        consent_id: "config:ingest.auto_concept_proposals".to_string(),
        concepts: suggestions
            .into_iter()
            .map(|(concept, confidence)| ProposedModelConcept {
                scheme_uri: concept.scheme_uri,
                pref_label: concept.pref_label,
                alt_labels: concept.alt_labels,
                definition: concept.definition,
                confidence,
            })
            .collect(),
    };
    propose(store, &input).map(Some)
}

fn heuristic_concept_confidence(text: &str, tags: &[String], concept: &Concept) -> f32 {
    let normalized_text = text.to_lowercase();
    let preferred_label = concept.pref_label.trim().to_lowercase();
    if !preferred_label.is_empty()
        && (normalized_text.contains(&preferred_label)
            || tags.iter().any(|tag| tag == &preferred_label))
    {
        return 0.95;
    }
    if concept.alt_labels.iter().any(|label| {
        let label = label.trim().to_lowercase();
        !label.is_empty() && normalized_text.contains(&label)
    }) {
        return 0.90;
    }
    let document_tokens = normalized_tokens(&normalized_text);
    let label_tokens = normalized_tokens(&preferred_label);
    if !label_tokens.is_empty()
        && label_tokens
            .iter()
            .all(|token| document_tokens.contains(token))
    {
        return 0.75;
    }
    0.0
}

fn normalized_tokens(text: &str) -> std::collections::HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn concept_suggestion_text(concept: &Concept) -> String {
    format!(
        "{}\n{}\n{}",
        concept.pref_label,
        concept.alt_labels.join(" "),
        concept.definition.as_deref().unwrap_or_default()
    )
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let dot = left
        .iter()
        .zip(right)
        .map(|(left_value, right_value)| left_value * right_value)
        .sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot / (left_norm * right_norm))
}

fn validate_proposal(input: &ModelEnrichmentProposalInput) -> Result<(), ShiroError> {
    let required = [
        ("provider", input.provider.as_str()),
        ("model", input.model.as_str()),
        ("actor_id", input.actor_id.as_str()),
        ("data_region", input.data_region.as_str()),
        ("retention_policy", input.retention_policy.as_str()),
        ("consent_id", input.consent_id.as_str()),
    ];
    if let Some((field, _)) = required.iter().find(|(_, value)| value.trim().is_empty()) {
        return Err(ShiroError::InvalidInput {
            message: format!("model enrichment requires {field}"),
        });
    }
    if input.concepts.is_empty() {
        return Err(ShiroError::InvalidInput {
            message: "model enrichment requires at least one proposed concept".to_string(),
        });
    }
    for concept in &input.concepts {
        if concept.scheme_uri.trim().is_empty()
            || concept.pref_label.trim().is_empty()
            || !(0.0..=1.0).contains(&concept.confidence)
        {
            return Err(ShiroError::InvalidInput {
                message: "proposed concepts require scheme, text label, and confidence in [0,1]"
                    .to_string(),
            });
        }
    }
    Ok(())
}
