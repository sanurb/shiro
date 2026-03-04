use std::sync::RwLock;

use shiro_core::ports::FtsIndex;
use shiro_core::{Segment, ShiroError};

/// In-memory brute-force index for development and testing.
pub struct MemoryIndex {
    segments: RwLock<Vec<Segment>>,
}

impl MemoryIndex {
    pub fn new() -> Self {
        Self {
            segments: RwLock::new(Vec::new()),
        }
    }
}

impl Default for MemoryIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl FtsIndex for MemoryIndex {
    fn index(&self, segments: &[Segment]) -> Result<(), ShiroError> {
        let mut store = self.segments.write().map_err(|e| ShiroError::Index {
            message: format!("lock poisoned: {e}"),
        })?;
        store.extend(segments.iter().cloned());
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<Segment>, ShiroError> {
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let store = self.segments.read().map_err(|e| ShiroError::Index {
            message: format!("lock poisoned: {e}"),
        })?;

        let query_lower = query.to_lowercase();
        let results = store
            .iter()
            .filter(|seg| seg.content.to_lowercase().contains(&query_lower))
            .take(limit)
            .cloned()
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::{DocId, SegmentId, Span};

    fn make_segment(doc_id: &DocId, index: usize, content: &str) -> Segment {
        Segment {
            id: SegmentId::new(doc_id, index),
            doc_id: doc_id.clone(),
            span: Span::new(0, content.len()).unwrap(),
            content: content.to_string(),
        }
    }

    #[test]
    fn index_and_search() {
        let idx = MemoryIndex::new();
        let doc_id = DocId::from_content(b"test");
        let segments = vec![
            make_segment(&doc_id, 0, "the quick brown fox"),
            make_segment(&doc_id, 1, "jumped over the lazy dog"),
            make_segment(&doc_id, 2, "rust is great"),
        ];

        idx.index(&segments).unwrap();

        let results = idx.search("fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("fox"));

        let results = idx.search("the", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_empty_index() {
        let idx = MemoryIndex::new();
        let results = idx.search("anything", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_case_insensitive() {
        let idx = MemoryIndex::new();
        let doc_id = DocId::from_content(b"test");
        let segments = vec![make_segment(&doc_id, 0, "hello world")];

        idx.index(&segments).unwrap();

        let results = idx.search("HELLO", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("hello"));
    }
}
