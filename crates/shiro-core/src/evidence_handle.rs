//! Stable public handles for canonical block evidence.

use serde::{Deserialize, Serialize};

use crate::{BlockGraph, DocId};

/// Content-derived block handle that is independent of segmentation.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct EvidenceHandleId(String);

impl EvidenceHandleId {
    /// Parse a persisted `blk_` evidence handle and reject malformed hexadecimal identities.
    pub fn from_stored(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let digest = value
            .strip_prefix("blk_")
            .ok_or_else(|| format!("evidence handle has invalid prefix or digest: {value}"))?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "evidence handle has invalid prefix or digest: {value}"
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the complete `blk_` evidence handle string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EvidenceHandleId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Derive the stable handle for a block.
///
/// Equal block text occurrences are disambiguated by their occurrence ordinal,
/// not byte span or operational block index. Segmenter-only changes therefore
/// cannot alter handles, and parser span changes preserve handles when block
/// text and occurrence identity remain stable.
pub fn evidence_handle_for_block(
    doc_id: &DocId,
    graph: &BlockGraph,
    block_index: usize,
) -> Option<EvidenceHandleId> {
    let block = graph.blocks.get(block_index)?;
    let occurrence = graph
        .reading_order
        .iter()
        .take_while(|index| index.0 != block_index)
        .filter(|index| {
            graph
                .blocks
                .get(index.0)
                .is_some_and(|candidate| candidate.canonical_text == block.canonical_text)
        })
        .count();
    let mut hasher = blake3::Hasher::new();
    hasher.update(doc_id.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(block.canonical_text.as_bytes());
    hasher.update(b"\0");
    hasher.update(&(occurrence as u64).to_le_bytes());
    Some(EvidenceHandleId(format!(
        "blk_{}",
        hasher.finalize().to_hex()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, BlockIdx, BlockKind, DocumentHeadingLevel, Span};

    fn graph(spans: [(usize, usize); 2]) -> BlockGraph {
        BlockGraph {
            blocks: vec![
                Block {
                    canonical_text: "same".to_string(),
                    rendered_text: None,
                    kind: BlockKind::Heading,
                    heading_level: Some(DocumentHeadingLevel::new(1).unwrap()),
                    span: Span::new(spans[0].0, spans[0].1).unwrap(),
                    source_locators: Vec::new(),
                },
                Block {
                    canonical_text: "same".to_string(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    heading_level: None,
                    span: Span::new(spans[1].0, spans[1].1).unwrap(),
                    source_locators: Vec::new(),
                },
            ],
            edges: Vec::new(),
            reading_order: vec![BlockIdx(0), BlockIdx(1)],
        }
    }

    #[test]
    fn handle_survives_span_changes_and_disambiguates_duplicate_text() {
        let doc_id = DocId::from_content(b"same same");
        let before = graph([(0, 4), (5, 9)]);
        let after = graph([(10, 14), (20, 24)]);
        let first = evidence_handle_for_block(&doc_id, &before, 0).unwrap();
        let second = evidence_handle_for_block(&doc_id, &before, 1).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            first,
            evidence_handle_for_block(&doc_id, &after, 0).unwrap()
        );
        assert_eq!(
            second,
            evidence_handle_for_block(&doc_id, &after, 1).unwrap()
        );
    }

    #[test]
    fn stored_handle_requires_a_hexadecimal_digest() {
        assert!(EvidenceHandleId::from_stored(format!("blk_{}", "a".repeat(64))).is_ok());
        assert!(EvidenceHandleId::from_stored(format!("blk_{}", "G".repeat(64))).is_err());
        assert!(EvidenceHandleId::from_stored(format!("blk_{}", "A".repeat(64))).is_err());
        assert!(EvidenceHandleId::from_stored("blk_short").is_err());
    }
}
