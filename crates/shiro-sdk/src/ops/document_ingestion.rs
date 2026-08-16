//! Canonical per-document ingestion lifecycle shared by add and batch ingest.

use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::ir::Segment;
use shiro_core::manifest::DocState;
use shiro_core::ports::Parser;
use shiro_core::{DocId, ShiroError};
use shiro_index::FtsIndex;
use shiro_parse::{segment_document, SEGMENTER_VERSION};
use shiro_store::Store;

/// A document whose canonical aggregate is staged and ready for FTS publication.
#[derive(Debug)]
pub(crate) struct StagedDocumentIngestion {
    pub doc_id: DocId,
    pub title: Option<String>,
    pub segments: Vec<Segment>,
    pub changed: bool,
}

/// Parse and atomically stage one document's canonical aggregate.
pub(crate) fn stage_document_bytes(
    store: &Store,
    parser: &dyn Parser,
    source_uri: &str,
    content: &[u8],
) -> Result<StagedDocumentIngestion, ShiroError> {
    let document = parser.parse(source_uri, content)?;
    let segments = segment_document(&document)?;
    let fingerprint =
        ProcessingFingerprint::new(parser.name(), parser.version(), SEGMENTER_VERSION);

    let changed = store.stage_document_processing(&document, &fingerprint, &segments)?;
    if !changed {
        let (stored_document, _) = store.get_document(&document.id)?;
        return Ok(StagedDocumentIngestion {
            doc_id: stored_document.id,
            title: stored_document.metadata.title,
            segments: Vec::new(),
            changed: false,
        });
    }

    store.set_state(&document.id, DocState::Indexing)?;
    tracing::info!(
        doc_id = %document.id,
        segments = segments.len(),
        "document ingestion staged canonical aggregate"
    );

    Ok(StagedDocumentIngestion {
        doc_id: document.id,
        title: document.metadata.title,
        segments,
        changed: true,
    })
}

/// Publish staged documents to FTS in one commit and mark each document Ready.
pub(crate) fn publish_staged_documents(
    store: &Store,
    fts: &FtsIndex,
    staged: &[&StagedDocumentIngestion],
) -> Result<(), ShiroError> {
    let changed: Vec<&StagedDocumentIngestion> = staged
        .iter()
        .copied()
        .filter(|document| document.changed)
        .collect();
    if changed.is_empty() {
        return Ok(());
    }

    let doc_ids: Vec<DocId> = changed
        .iter()
        .map(|document| document.doc_id.clone())
        .collect();
    let segments: Vec<Segment> = changed
        .iter()
        .flat_map(|document| document.segments.iter().cloned())
        .collect();

    if let Err(error) = fts.replace_document_segments(&doc_ids, &segments) {
        fail_staged_documents(store, fts, &doc_ids, &error);
        return Err(error);
    }

    if let Err(error) = store.set_documents_ready(&doc_ids) {
        fail_staged_documents(store, fts, &doc_ids, &error);
        return Err(error);
    }

    for document in changed {
        tracing::info!(
            doc_id = %document.doc_id,
            segments = document.segments.len(),
            "document ingestion reached READY"
        );
    }

    Ok(())
}

/// Parse, stage, publish, and finalize one document.
pub(crate) fn ingest_document_bytes(
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    source_uri: &str,
    content: &[u8],
) -> Result<StagedDocumentIngestion, ShiroError> {
    let staged = stage_document_bytes(store, parser, source_uri, content)?;
    publish_staged_documents(store, fts, &[&staged])?;
    Ok(staged)
}

fn fail_staged_documents(
    store: &Store,
    fts: &FtsIndex,
    doc_ids: &[DocId],
    ingestion_error: &ShiroError,
) {
    if let Err(cleanup_error) = fts.replace_document_segments(doc_ids, &[]) {
        tracing::warn!(
            error = %cleanup_error,
            "document ingestion failed to remove derived FTS entries"
        );
    }
    for doc_id in doc_ids {
        if let Err(state_error) = store.set_state(doc_id, DocState::Failed) {
            tracing::error!(
                doc_id = %doc_id,
                error = %state_error,
                original_error = %ingestion_error,
                "document ingestion failed to mark document FAILED"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_parse::PlainTextParser;

    fn test_runtime() -> (tempfile::TempDir, Store, FtsIndex) {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap();
        let store = Store::open(&root.join("shiro.db")).unwrap();
        let fts = FtsIndex::open(&root.join("tantivy")).unwrap();
        (dir, store, fts)
    }

    #[test]
    fn ready_document_ingestion_is_idempotent() {
        let (_dir, store, fts) = test_runtime();
        let parser = PlainTextParser;
        let content = b"Idempotent ingestion\n\nCanonical content";

        let first = ingest_document_bytes(&store, &fts, &parser, "test.txt", content).unwrap();
        let second = ingest_document_bytes(&store, &fts, &parser, "test.txt", content).unwrap();

        assert!(first.changed);
        assert!(!second.changed);
        assert!(second.segments.is_empty());
        let (_, state) = store.get_document(&first.doc_id).unwrap();
        assert_eq!(state, DocState::Ready);
        assert!(store.get_fingerprint(&first.doc_id).unwrap().is_some());
        assert_eq!(fts.num_segments().unwrap(), first.segments.len() as u64);
    }

    #[test]
    fn failed_document_retry_repairs_document() {
        let (_dir, store, fts) = test_runtime();
        let parser = PlainTextParser;
        let content = b"Recoverable ingestion\n\nRetry this document";

        let staged = stage_document_bytes(&store, &parser, "retry.txt", content).unwrap();
        store.set_state(&staged.doc_id, DocState::Failed).unwrap();

        let (failed_document, failed_state) = store.get_document(&staged.doc_id).unwrap();
        assert_eq!(failed_state, DocState::Failed);
        assert!(!failed_document.blocks.blocks.is_empty());
        assert!(store.get_fingerprint(&staged.doc_id).unwrap().is_some());
        assert!(!store.get_segments(&staged.doc_id).unwrap().is_empty());

        let repaired = ingest_document_bytes(&store, &fts, &parser, "retry.txt", content).unwrap();

        assert!(repaired.changed);
        let (_, repaired_state) = store.get_document(&staged.doc_id).unwrap();
        assert_eq!(repaired_state, DocState::Ready);
        assert!(!fts.search("retry", 10).unwrap().is_empty());
    }
}
