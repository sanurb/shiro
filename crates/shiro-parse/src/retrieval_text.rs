//! Versioned, bounded retrieval text derived from canonical blocks.
//!
//! Source-faithful segment bodies retain exact canonical spans. Retrieval text
//! adds deterministic title/heading context without changing canonical data.

use shiro_core::{
    BlockGraph, BlockIdx, BlockKind, Document, DocumentHeadingLevel, Segment, SegmentId,
    ShiroError, Span,
};

pub const RETRIEVAL_TEXT_VERSION: u32 = 1;
pub const MAX_RETRIEVAL_TEXT_BYTES: usize = 2_048;
const MAX_PREFIX_BYTES: usize = 512;
const MIN_BODY_BYTES: usize = MAX_RETRIEVAL_TEXT_BYTES - MAX_PREFIX_BYTES;

pub(crate) fn derive_segments(
    document: &Document,
    graph: &BlockGraph,
) -> Result<Vec<Segment>, ShiroError> {
    let mut segments = Vec::new();
    let mut heading_path: Vec<(Option<DocumentHeadingLevel>, String)> = Vec::new();

    for block_index in &graph.reading_order {
        let BlockIdx(index) = *block_index;
        let block = graph
            .blocks
            .get(index)
            .ok_or_else(|| ShiroError::InvalidIr {
                message: format!(
                    "block index {index} out of range (len={})",
                    graph.blocks.len()
                ),
            })?;
        if block.kind == BlockKind::Heading {
            match block.heading_level {
                Some(level) => heading_path.retain(|(active_level, _)| {
                    active_level.is_some_and(|active_level| active_level < level)
                }),
                None => heading_path.clear(),
            }
            heading_path.push((
                block.heading_level,
                normalize_context(&block.canonical_text),
            ));
        }

        let source = document
            .canonical_text
            .get(block.span.start()..block.span.end())
            .ok_or_else(|| ShiroError::InvalidIr {
                message: format!("block {index} span is not a canonical UTF-8 boundary"),
            })?;
        let trimmed = source.trim();
        if trimmed.is_empty() {
            continue;
        }
        let trim_start = trimmed.as_ptr() as usize - source.as_ptr() as usize;
        let canonical_start = block.span.start() + trim_start;
        let prefix = retrieval_prefix(document, &heading_path);
        let body_budget = MAX_RETRIEVAL_TEXT_BYTES
            .saturating_sub(prefix.len())
            .max(MIN_BODY_BYTES);

        for (chunk_start, chunk_end) in bounded_chunks(trimmed, body_budget) {
            let body = &trimmed[chunk_start..chunk_end];
            let span = Span::new(canonical_start + chunk_start, canonical_start + chunk_end)
                .map_err(|error| ShiroError::InvalidIr {
                    message: format!("retrieval text canonical span invalid: {error}"),
                })?;
            let retrieval_text = if prefix.is_empty() {
                body.to_string()
            } else {
                format!("{prefix}{body}")
            };
            if retrieval_text.len() > MAX_RETRIEVAL_TEXT_BYTES {
                return Err(ShiroError::InvalidIr {
                    message: format!(
                        "retrieval text byte budget exceeded: {} > {MAX_RETRIEVAL_TEXT_BYTES}",
                        retrieval_text.len()
                    ),
                });
            }
            let segment_index = segments.len();
            segments.push(Segment {
                id: SegmentId::new(&document.id, segment_index),
                doc_id: document.id.clone(),
                index: segment_index,
                span,
                body: body.to_string(),
                retrieval_text,
            });
        }
    }

    Ok(segments)
}

fn retrieval_prefix(
    document: &Document,
    heading_path: &[(Option<DocumentHeadingLevel>, String)],
) -> String {
    let title = document
        .metadata
        .title
        .as_deref()
        .map(normalize_context)
        .filter(|title| !title.is_empty());
    let section = (!heading_path.is_empty()).then(|| {
        heading_path
            .iter()
            .map(|(_, heading)| heading.as_str())
            .collect::<Vec<_>>()
            .join(" > ")
    });

    let mut prefix = String::new();
    if let Some(title) = title {
        prefix.push_str("Document: ");
        prefix.push_str(&truncate_utf8(&title, 192));
        prefix.push('\n');
    }
    if let Some(section) = section {
        prefix.push_str("Section: ");
        prefix.push_str(&truncate_utf8(&section, 280));
        prefix.push('\n');
    }
    if !prefix.is_empty() {
        prefix.push('\n');
    }
    truncate_utf8(&prefix, MAX_PREFIX_BYTES)
}

fn normalize_context(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn bounded_chunks(text: &str, max_bytes: usize) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        while start < text.len()
            && text[start..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            start += text[start..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
        }
        if start >= text.len() {
            break;
        }
        let mut end = (start + max_bytes).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end < text.len() {
            if let Some(relative) = text[start..end].rfind(char::is_whitespace) {
                if relative > max_bytes / 2 {
                    end = start + relative;
                }
            }
        }
        if end == start {
            end = start
                + text[start..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
        }
        chunks.push((start, end));
        start = end;
    }
    chunks
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::{Block, DocId, Metadata};

    #[test]
    fn title_and_heading_path_are_bounded_without_changing_source_body() {
        let canonical = "# Alpha\n\n## Beta\n\nimportant evidence";
        let document = Document {
            id: DocId::from_content(canonical.as_bytes()),
            canonical_text: canonical.to_string(),
            rendered_text: None,
            metadata: Metadata {
                title: Some("Example".to_string()),
                source_uri: "test.md".to_string(),
                source_hash: "hash".to_string(),
            },
            blocks: BlockGraph {
                blocks: vec![
                    Block {
                        canonical_text: "Alpha".to_string(),
                        rendered_text: None,
                        kind: BlockKind::Heading,
                        heading_level: Some(DocumentHeadingLevel::new(1).unwrap()),
                        span: Span::new(0, 7).unwrap(),
                        source_locators: Vec::new(),
                    },
                    Block {
                        canonical_text: "Beta".to_string(),
                        rendered_text: None,
                        kind: BlockKind::Heading,
                        heading_level: Some(DocumentHeadingLevel::new(2).unwrap()),
                        span: Span::new(9, 16).unwrap(),
                        source_locators: Vec::new(),
                    },
                    Block {
                        canonical_text: "important evidence".to_string(),
                        rendered_text: None,
                        kind: BlockKind::Paragraph,
                        heading_level: None,
                        span: Span::new(18, canonical.len()).unwrap(),
                        source_locators: Vec::new(),
                    },
                ],
                edges: Vec::new(),
                reading_order: vec![BlockIdx(0), BlockIdx(1), BlockIdx(2)],
            },
            losses: Vec::new(),
        };

        let segments = derive_segments(&document, &document.blocks).unwrap();
        let evidence = segments.last().unwrap();
        assert_eq!(evidence.body, "important evidence");
        assert!(evidence.retrieval_text.contains("Document: Example"));
        assert!(evidence.retrieval_text.contains("Section: Alpha > Beta"));
        assert!(segments
            .iter()
            .all(|segment| segment.retrieval_text.len() <= MAX_RETRIEVAL_TEXT_BYTES));
    }

    #[test]
    fn oversized_unicode_block_splits_on_valid_canonical_spans() {
        let body = "évidence ".repeat(600);
        let document = Document {
            id: DocId::from_content(body.as_bytes()),
            canonical_text: body.clone(),
            rendered_text: None,
            metadata: Metadata {
                title: None,
                source_uri: "large.txt".to_string(),
                source_hash: "hash".to_string(),
            },
            blocks: BlockGraph {
                blocks: vec![Block {
                    canonical_text: body.clone(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    heading_level: None,
                    span: Span::new(0, body.len()).unwrap(),
                    source_locators: Vec::new(),
                }],
                edges: Vec::new(),
                reading_order: vec![BlockIdx(0)],
            },
            losses: Vec::new(),
        };

        let segments = derive_segments(&document, &document.blocks).unwrap();
        assert!(segments.len() > 1);
        for segment in segments {
            assert_eq!(
                &document.canonical_text[segment.span.start()..segment.span.end()],
                segment.body
            );
            assert!(segment.retrieval_text.len() <= MAX_RETRIEVAL_TEXT_BYTES);
        }
    }
}
