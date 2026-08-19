//! Canonical per-document ingestion lifecycle shared by add and batch ingest.

use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::ir::Segment;
use shiro_core::manifest::DocState;
use shiro_core::ports::Parser;
use shiro_core::{DocId, ShiroError, WriteProvenance};
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
    stage_document_bytes_with_force(store, parser, source_uri, content, false)
}

pub(crate) fn stage_document_bytes_with_force(
    store: &Store,
    parser: &dyn Parser,
    source_uri: &str,
    content: &[u8],
    force: bool,
) -> Result<StagedDocumentIngestion, ShiroError> {
    let document = parser.parse(source_uri, content)?;
    let segments = segment_document(&document)?;
    let fingerprint =
        ProcessingFingerprint::new(parser.name(), parser.version(), SEGMENTER_VERSION);

    let provenance =
        WriteProvenance::local_user("document_ingestion", document.metadata.source_hash.clone());
    let changed = store.stage_document_processing_with_force(
        &document,
        &fingerprint,
        &segments,
        content,
        &provenance,
        force,
    )?;
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

/// Parse and atomically stage one remotely acquired document with URL evidence.
pub(crate) fn stage_url_document_bytes(
    store: &Store,
    parser: &dyn Parser,
    source_uri: &str,
    content: &[u8],
    acquisition: &shiro_store::UrlAcquisitionRecord,
) -> Result<StagedDocumentIngestion, ShiroError> {
    let document = parser.parse(source_uri, content)?;
    let segments = segment_document(&document)?;
    let fingerprint =
        ProcessingFingerprint::new(parser.name(), parser.version(), SEGMENTER_VERSION);
    let provenance =
        WriteProvenance::local_user("url_acquisition", document.metadata.source_hash.clone());
    let changed = store.stage_url_document_processing(
        &document,
        &fingerprint,
        &segments,
        content,
        &provenance,
        acquisition,
    )?;
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

    // The active vector artifact represents the previous corpus. Deactivate it
    // atomically before mutating FTS so hybrid reads never mix corpus versions.
    store.begin_incremental_fts_publication()?;

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
    use shiro_core::ir::{LossKind, ParseLoss};
    use shiro_parse::{MarkdownParser, PlainTextParser};

    struct LossyTestParser;

    impl Parser for LossyTestParser {
        fn name(&self) -> &str {
            "lossy_test"
        }

        fn version(&self) -> u32 {
            1
        }

        fn parse(
            &self,
            source_uri: &str,
            content: &[u8],
        ) -> Result<shiro_core::Document, ShiroError> {
            let mut document = PlainTextParser.parse(source_uri, content)?;
            document.losses.push(ParseLoss {
                kind: LossKind::Image,
                span: None,
                message: "test image omitted".to_string(),
            });
            Ok(document)
        }
    }

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
    fn url_acquisition_evidence_is_atomic_with_canonical_staging() {
        let (_tmp, store, fts) = test_runtime();
        let bytes = b"remote canonical evidence";
        let content_hash = blake3::hash(bytes).to_hex().to_string();
        let acquisition = shiro_store::UrlAcquisitionRecord {
            requested_url: "https://example.com/start".to_string(),
            final_url: "https://example.com/final.txt".to_string(),
            redirects_json: r#"["https://example.com/final.txt"]"#.to_string(),
            content_type: Some("text/plain".to_string()),
            signature: "plaintext_utf8".to_string(),
            byte_count: bytes.len(),
            content_hash,
        };
        let staged = stage_url_document_bytes(
            &store,
            &PlainTextParser,
            &acquisition.final_url,
            bytes,
            &acquisition,
        )
        .unwrap();
        publish_staged_documents(&store, &fts, &[&staged]).unwrap();
        let loaded = store.get_url_acquisitions(&staged.doc_id).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].requested_url, acquisition.requested_url);
        assert_eq!(loaded[0].signature, "plaintext_utf8");
    }

    #[test]
    fn ingestion_atomically_persists_source_bytes_and_parse_losses() {
        let (_dir, store, fts) = test_runtime();
        let content = b"source artifact with parse loss";

        let ingested =
            ingest_document_bytes(&store, &fts, &LossyTestParser, "lossy-source.txt", content)
                .unwrap();

        let (loaded, state) = store.get_document(&ingested.doc_id).unwrap();
        assert_eq!(state, DocState::Ready);
        assert_eq!(loaded.losses.len(), 1);
        assert_eq!(loaded.losses[0].kind, LossKind::Image);
        assert_eq!(loaded.losses[0].message, "test image omitted");
        assert!(store.blob_exists(&loaded.metadata.source_hash).unwrap());
        assert_eq!(
            store.get_blob(&loaded.metadata.source_hash).unwrap(),
            content
        );
        let provenance = store.get_document_provenance(&ingested.doc_id).unwrap();
        assert_eq!(provenance.len(), 2);
        assert!(provenance
            .iter()
            .all(|record| record.actor_kind == shiro_core::ProvenanceActorKind::Human));
        assert!(provenance
            .iter()
            .all(|record| record.content_hash == loaded.metadata.source_hash));
        assert!(provenance
            .iter()
            .any(|record| record.operation == "document_ingestion"));
    }

    #[test]
    fn ready_document_with_changed_processing_fingerprint_is_reprocessed() {
        let (_dir, store, fts) = test_runtime();
        let content = b"# Processing Drift\n\nCanonical body";

        let first =
            ingest_document_bytes(&store, &fts, &PlainTextParser, "drift.txt", content).unwrap();
        let second =
            ingest_document_bytes(&store, &fts, &MarkdownParser, "drift.md", content).unwrap();

        assert!(first.changed);
        assert!(second.changed);
        assert_eq!(first.doc_id, second.doc_id);
        let fingerprint = store.get_fingerprint(&second.doc_id).unwrap().unwrap();
        assert_eq!(fingerprint.parser_name, "markdown");
        let (document, state) = store.get_document(&second.doc_id).unwrap();
        assert_eq!(state, DocState::Ready);
        assert_eq!(
            document.blocks.blocks[0].kind,
            shiro_core::BlockKind::Heading
        );
        assert_eq!(store.count_versions(&second.doc_id).unwrap(), 2);
        assert_eq!(fts.num_segments().unwrap(), second.segments.len() as u64);
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
