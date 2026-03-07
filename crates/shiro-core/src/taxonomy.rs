//! SKOS-style taxonomy types for concept tagging.
//!
//! Concepts are identified by content-addressed [`ConceptId`]s derived from
//! their scheme URI and preferred label, enabling stable cross-system references.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Content-addressed concept identifier (`con_<blake3_hex>`).
///
/// Derived from `blake3(scheme_uri:pref_label)`, so two concepts with the same
/// scheme and label always produce the same id regardless of system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConceptId(String);

impl ConceptId {
    /// Derive a [`ConceptId`] from a scheme URI and preferred label.
    pub fn new(scheme_uri: &str, pref_label: &str) -> Self {
        let input = format!("{scheme_uri}:{pref_label}");
        let hex = blake3::hash(input.as_bytes()).to_hex();
        Self(format!("con_{hex}"))
    }

    /// Reconstruct from a stored string, validating the `con_` prefix.
    pub fn from_stored(s: impl Into<String>) -> Result<Self, &'static str> {
        let s = s.into();
        if s.starts_with("con_") {
            Ok(Self(s))
        } else {
            Err("ConceptId must start with 'con_'")
        }
    }

    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConceptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A taxonomy concept, analogous to a SKOS `skos:Concept`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concept {
    /// Content-addressed identifier.
    pub id: ConceptId,
    /// URI of the scheme this concept belongs to (e.g. a SKOS `ConceptScheme`).
    pub scheme_uri: String,
    /// Preferred human-readable label.
    pub pref_label: String,
    /// Alternative labels (synonyms, abbreviations).
    pub alt_labels: Vec<String>,
    /// Optional prose definition.
    pub definition: Option<String>,
}

/// SKOS hierarchical/associative relation types.
///
/// `Broader` and `Narrower` are inverses: if A is `Broader` than B, then B is
/// `Narrower` than A.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SkosRelation {
    /// The target concept is more general.
    Broader,
    /// The target concept is more specific (inverse of [`Broader`](SkosRelation::Broader)).
    Narrower,
    /// The target concept is associatively related.
    Related,
}

/// A directed relation between two concepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptRelation {
    /// Source concept.
    pub from: ConceptId,
    /// Target concept.
    pub to: ConceptId,
    /// Type of relation.
    pub relation: SkosRelation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concept_id_deterministic() {
        let a = ConceptId::new("http://example.org/scheme", "Rust");
        let b = ConceptId::new("http://example.org/scheme", "Rust");
        assert_eq!(a, b);
    }

    #[test]
    fn concept_id_has_prefix() {
        let id = ConceptId::new("http://example.org", "test");
        assert!(id.as_str().starts_with("con_"));
    }

    #[test]
    fn concept_id_differs_by_label() {
        let a = ConceptId::new("http://example.org", "alpha");
        let b = ConceptId::new("http://example.org", "beta");
        assert_ne!(a, b);
    }

    #[test]
    fn concept_id_differs_by_scheme() {
        let a = ConceptId::new("http://a.org", "label");
        let b = ConceptId::new("http://b.org", "label");
        assert_ne!(a, b);
    }

    #[test]
    fn from_stored_validates_prefix() {
        assert!(ConceptId::from_stored("con_abc123").is_ok());
        assert!(ConceptId::from_stored("bad_abc123").is_err());
    }

    #[test]
    fn empty_inputs_are_valid() {
        let id = ConceptId::new("", "");
        assert!(id.as_str().starts_with("con_"));
    }

    #[test]
    fn skos_relation_serde() {
        let json = serde_json::to_string(&SkosRelation::Broader).unwrap();
        assert_eq!(json, "\"BROADER\"");
        let back: SkosRelation = serde_json::from_str("\"NARROWER\"").unwrap();
        assert_eq!(back, SkosRelation::Narrower);
    }

    #[test]
    fn concept_json_roundtrip() {
        let c = Concept {
            id: ConceptId::new("http://example.org", "Rust"),
            scheme_uri: "http://example.org".to_string(),
            pref_label: "Rust".to_string(),
            alt_labels: vec!["rust-lang".to_string()],
            definition: Some("A systems programming language".to_string()),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Concept = serde_json::from_str(&json).unwrap();
        assert_eq!(c.id, back.id);
    }
    mod proptests {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn concept_id_deterministic_for_any_inputs(
                scheme in "[a-z]{1,20}",
                label in "[a-z]{1,20}",
            ) {
                let a = ConceptId::new(&scheme, &label);
                let b = ConceptId::new(&scheme, &label);
                prop_assert_eq!(a.clone(), b);
                prop_assert!(a.as_str().starts_with("con_"));
            }
        }
    }
}
