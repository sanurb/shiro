//! `enrich` — run heuristic enrichment on a document.

use serde::{Deserialize, Serialize};
use shiro_core::enrichment::EnrichmentResult;
use shiro_core::ShiroError;
use shiro_store::Store;

use super::read::resolve_doc_id;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnrichInput {
    pub doc_id: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnrichOutput {
    pub doc_id: String,
    pub title: Option<String>,
    pub summary_length: usize,
    pub tags: Vec<String>,
}

pub fn execute(store: &Store, input: &EnrichInput) -> Result<EnrichOutput, ShiroError> {
    let doc_id = resolve_doc_id(store, &input.doc_id)?;
    let (doc, state) = store.get_document(&doc_id)?;

    if state.as_str() != "READY" {
        return Err(ShiroError::InvalidInput {
            message: format!(
                "document {} is in state {} — must be READY for enrichment",
                doc_id,
                state.as_str()
            ),
        });
    }

    let text = doc.rendered_text.as_deref().unwrap_or(&doc.canonical_text);

    let title = extract_title(text);
    let summary = build_summary(text);
    let tags = extract_tags(text);

    let content_hash = blake3::hash(doc.canonical_text.as_bytes())
        .to_hex()
        .to_string();
    let created_at = timestamp_now();

    let summary_length = summary.as_ref().map(|s| s.len()).unwrap_or(0);

    let enrichment = EnrichmentResult {
        doc_id: doc_id.clone(),
        title: title.clone(),
        summary,
        tags: tags.clone(),
        concepts: vec![],
        provider: "heuristic".to_string(),
        content_hash,
        created_at,
    };

    store.put_enrichment(&enrichment)?;

    Ok(EnrichOutput {
        doc_id: doc_id.as_str().to_string(),
        title,
        summary_length,
        tags,
    })
}

// ---------------------------------------------------------------------------
// Helpers (pub(crate) for potential reuse)
// ---------------------------------------------------------------------------

/// Extract a title from text: first heading or first non-empty line.
pub(crate) fn extract_title(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim();
            if !heading.is_empty() {
                return Some(heading.to_string());
            }
        }
        return Some(trimmed.to_string());
    }
    None
}

/// Extract tags from markdown headings (lowercased, deduplicated).
pub(crate) fn extract_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim().to_lowercase();
            if !heading.is_empty() && seen.insert(heading.clone()) {
                tags.push(heading);
            }
        }
    }
    tags
}

/// Build a summary by taking the first ~500 characters of text.
fn build_summary(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if text.len() <= 500 {
        return Some(text.to_string());
    }
    let end = text
        .char_indices()
        .take(500)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(text.len());
    Some(text[..end].to_string())
}

/// Minimal UTC timestamp (seconds precision) without external crates.
pub(crate) fn timestamp_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", d.as_secs())
}
