use camino::Utf8Path;
use shiro_core::ports::Parser;
use shiro_core::{DocId, Document, Metadata, Segment, SegmentId, ShiroError, Span};

/// Plain-text parser that splits content on double newlines into paragraph segments.
pub struct PlainTextParser;

impl Parser for PlainTextParser {
    fn parse(&self, path: &Utf8Path, content: &[u8]) -> Result<Document, ShiroError> {
        let text = std::str::from_utf8(content).map_err(|e| ShiroError::Parse {
            message: format!("invalid UTF-8: {e}"),
        })?;

        let doc_id = DocId::from_content(content);
        let mut segments = Vec::new();

        for part in text.split("\n\n") {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }

            let seg_index = segments.len();

            // Compute byte offset of `part` within `text`.
            let start = part.as_ptr() as usize - text.as_ptr() as usize;
            let end = start + part.len();

            let span = Span::new(start, end).map_err(|e| ShiroError::Parse {
                message: e.to_string(),
            })?;

            segments.push(Segment {
                id: SegmentId::new(&doc_id, seg_index),
                doc_id: doc_id.clone(),
                span,
                content: trimmed.to_owned(),
            });
        }

        Ok(Document {
            id: doc_id,
            metadata: Metadata {
                title: None,
                source: path.to_owned(),
            },
            segments,
            blocks: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8Path;

    #[test]
    fn parse_empty_content() {
        let parser = PlainTextParser;
        let doc = parser.parse(Utf8Path::new("empty.txt"), b"").unwrap();
        assert!(doc.segments.is_empty());
    }

    #[test]
    fn parse_single_paragraph() {
        let parser = PlainTextParser;
        let content = b"Hello, world!";
        let doc = parser.parse(Utf8Path::new("single.txt"), content).unwrap();
        assert_eq!(doc.segments.len(), 1);
        assert_eq!(doc.segments[0].content, "Hello, world!");
        assert_eq!(doc.segments[0].span.start(), 0);
        assert_eq!(doc.segments[0].span.end(), content.len());
    }

    #[test]
    fn parse_multiple_paragraphs() {
        let parser = PlainTextParser;
        let content = b"First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let doc = parser.parse(Utf8Path::new("multi.txt"), content).unwrap();
        assert_eq!(doc.segments.len(), 3);
        assert_eq!(doc.segments[0].content, "First paragraph.");
        assert_eq!(doc.segments[1].content, "Second paragraph.");
        assert_eq!(doc.segments[2].content, "Third paragraph.");

        // Verify spans are non-overlapping and ordered.
        for pair in doc.segments.windows(2) {
            assert!(pair[0].span.end() <= pair[1].span.start());
        }
    }

    #[test]
    fn parse_invalid_utf8() {
        let parser = PlainTextParser;
        let bad = &[0xFF, 0xFE, 0x80];
        let err = parser.parse(Utf8Path::new("bad.bin"), bad).unwrap_err();
        assert!(matches!(err, ShiroError::Parse { .. }));
    }
}
