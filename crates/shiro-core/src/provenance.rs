//! Immutable write provenance and trust-zone vocabulary.

use serde::{Deserialize, Serialize};

/// Structured identity class for the initiator of a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceActorKind {
    Human,
    System,
    Agent,
}

impl ProvenanceActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "HUMAN",
            Self::System => "SYSTEM",
            Self::Agent => "AGENT",
        }
    }
}

/// Origin and verification class controlling default retrieval visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrustZone {
    Canonical,
    Derived,
    Proposed,
    Quarantined,
}

impl TrustZone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "CANONICAL",
            Self::Derived => "DERIVED",
            Self::Proposed => "PROPOSED",
            Self::Quarantined => "QUARANTINED",
        }
    }
}

/// Provenance supplied by a caller for one immutable write activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteProvenance {
    pub actor_kind: ProvenanceActorKind,
    pub actor_id: String,
    pub operation: String,
    pub content_hash: String,
}

impl WriteProvenance {
    /// Construct provenance for an explicit local-user operation.
    pub fn local_user(operation: impl Into<String>, content_hash: impl Into<String>) -> Self {
        Self {
            actor_kind: ProvenanceActorKind::Human,
            actor_id: "local_user".to_string(),
            operation: operation.into(),
            content_hash: content_hash.into(),
        }
    }
}

/// Queryable immutable provenance record loaded from the authoritative store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProvenanceRecord {
    pub provenance_id: i64,
    pub actor_kind: ProvenanceActorKind,
    pub actor_id: String,
    pub operation: String,
    pub content_hash: String,
    pub created_at: String,
}
