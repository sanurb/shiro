//! Evaluation-only adapter for the audited pdf-inspector revision.
//!
//! This executable emits a versioned JSON inspection artifact. It does not
//! produce a Shiro canonical document or publish parser output.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::process::ExitCode;

use camino::{Utf8Path, Utf8PathBuf};
use pdf_inspector::{
    DetectionConfig, PdfError, PdfOptions, PdfProcessResult, PdfType, ProcessMode, ScanStrategy,
};
use serde::{Deserialize, Serialize};

const PDF_INSPECTOR_CRATE_VERSION: &str = "1.14.2";
const PDF_INSPECTOR_SOURCE_REVISION: &str = "2543abe3715848589903754f30b5dca54f6b33a6";
const PDF_INSPECTOR_PROBE_ADAPTER_VERSION: u32 = 1;
const PDF_INSPECTOR_PROBE_SCHEMA_VERSION: u32 = 1;
const PDF_INSPECTOR_PROBE_MAX_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
const PDF_INSPECTOR_PROBE_MAX_REPORT_PAGES: u32 = 10_000;

/// Discriminated JSON protocol envelope emitted by the PDF inspector probe.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PdfInspectorProbeEnvelope {
    /// Successful PDF inspection with no canonical or extracted document text.
    Success { report: PdfInspectorProbeReport },
    /// Typed terminal failure produced before any Shiro publication.
    Error { error: PdfInspectorProbeFailure },
}

/// Versioned routing and layout evidence from one immutable PDF source.
#[derive(Debug, Serialize, Deserialize)]
struct PdfInspectorProbeReport {
    /// Probe protocol schema version, independent of the upstream crate version.
    schema_version: u32,
    /// Content identity and admitted source size.
    source: PdfInspectorSourceIdentity,
    /// Exact inspector and Shiro adapter identity.
    inspector: PdfInspectorProbeFingerprint,
    /// Detector, layout, and page-routing observations.
    inspection: PdfInspectorInspection,
}

/// Content identity for the exact bytes inspected by pdf-inspector.
#[derive(Debug, Serialize, Deserialize)]
struct PdfInspectorSourceIdentity {
    /// Lowercase BLAKE3 digest of the complete admitted source bytes.
    blake3: String,
    /// Number of admitted source bytes.
    byte_len: u64,
}

/// Complete implementation identity for reproducible PDF probe artifacts.
#[derive(Debug, Serialize, Deserialize)]
struct PdfInspectorProbeFingerprint {
    /// Upstream crate version declared by the audited source revision.
    crate_version: String,
    /// Exact audited Git source revision.
    source_revision: String,
    /// Shiro-owned JSON adapter version.
    adapter_version: u32,
    /// Fixed probe profile; full-page detection and layout analysis, without Markdown.
    profile: PdfInspectorProbeProfile,
}

/// Stable PDF probe profile rather than mutable upstream defaults.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PdfInspectorProbeProfile {
    /// Scan every page and analyze layout without rendering Markdown.
    FullPageLayoutAnalysis,
}

/// Parser-neutral subset of pdf-inspector observations retained for evaluation.
#[derive(Debug, Serialize, Deserialize)]
struct PdfInspectorInspection {
    /// Heuristic PDF class reported by pdf-inspector.
    pdf_type: PdfInspectorDocumentType,
    /// Number of physical pages reported by the parsed PDF page tree.
    page_count: u32,
    /// Upstream routing heuristic score; this is not parse-quality confidence.
    detector_confidence: f32,
    /// Wall time measured inside pdf-inspector for this analysis attempt.
    processing_time_ms: u64,
    /// Whether pdf-inspector detected broken font encodings in extracted text.
    has_encoding_issues: bool,
    /// Whether tables or multiple columns were observed anywhere in the PDF.
    has_complex_layout: bool,
    /// Whole-document route derived only from explicit per-page OCR directives.
    recommended_route: PdfInspectorDocumentRoute,
    /// One record per physical page in ascending one-based page order.
    pages: Vec<PdfInspectorPageEvidence>,
}

/// Stable parser-neutral spelling of pdf-inspector's document classes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PdfInspectorDocumentType {
    TextBased,
    Scanned,
    ImageBased,
    Mixed,
}

/// Whole-document route derived from page-level OCR requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PdfInspectorDocumentRoute {
    NativeExtract,
    Ocr,
    MixedPageRouting,
}

/// Page-level routing and layout evidence from pdf-inspector.
#[derive(Debug, Serialize, Deserialize)]
struct PdfInspectorPageEvidence {
    /// One-based physical PDF page number.
    page_number: u32,
    /// Whether rectangle or heuristic table evidence was detected on this page.
    has_table_geometry: bool,
    /// Whether multiple text columns were detected on this page.
    has_multiple_columns: bool,
    /// Explicit upstream directive that this page requires OCR.
    needs_ocr: bool,
    /// Machine-readable upstream reasons that this page needs OCR.
    ocr_reasons: Vec<String>,
    /// Conservative route: native extraction unless pdf-inspector requires OCR.
    recommended_route: PdfInspectorPageRoute,
}

/// Conservative page route supported by the current evaluation probe.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PdfInspectorPageRoute {
    NativeExtract,
    Ocr,
}

/// Stable failure returned by the standalone PDF inspector protocol.
#[derive(Debug, Serialize, Deserialize)]
struct PdfInspectorProbeFailure {
    /// Machine-readable failure category.
    code: PdfInspectorProbeFailureCode,
    /// Redacted message with a unique searchable literal prefix.
    message: String,
}

/// Failure categories that callers may use for evaluation accounting.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PdfInspectorProbeFailureCode {
    InvalidArguments,
    SourceIo,
    SourceLimitExceeded,
    NotPdf,
    EncryptedPdf,
    InvalidPdfStructure,
    PdfParse,
    InvalidInspectorOutput,
    ReportLimitExceeded,
    ProtocolWrite,
}

fn main() -> ExitCode {
    let envelope = match parse_pdf_source_argument().and_then(run_pdf_inspector_probe) {
        Ok(report) => PdfInspectorProbeEnvelope::Success { report },
        Err(error) => PdfInspectorProbeEnvelope::Error { error },
    };
    let success = matches!(envelope, PdfInspectorProbeEnvelope::Success { .. });

    match write_probe_envelope(&envelope) {
        Ok(()) if success => ExitCode::SUCCESS,
        Ok(()) => ExitCode::from(1),
        Err(error) => {
            let failure = PdfInspectorProbeFailure {
                code: PdfInspectorProbeFailureCode::ProtocolWrite,
                message: format!("PDF inspector probe protocol write failed: {error}"),
            };
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{}", failure.message);
            ExitCode::from(1)
        }
    }
}

fn parse_pdf_source_argument() -> Result<Utf8PathBuf, PdfInspectorProbeFailure> {
    let mut arguments = std::env::args_os();
    let _program_name = arguments.next();
    let source = arguments.next().ok_or_else(|| PdfInspectorProbeFailure {
        code: PdfInspectorProbeFailureCode::InvalidArguments,
        message: "PDF inspector probe arguments invalid: expected exactly one UTF-8 PDF path"
            .to_string(),
    })?;
    if arguments.next().is_some() {
        return Err(PdfInspectorProbeFailure {
            code: PdfInspectorProbeFailureCode::InvalidArguments,
            message: "PDF inspector probe arguments invalid: expected exactly one UTF-8 PDF path"
                .to_string(),
        });
    }
    Utf8PathBuf::from_path_buf(source.into()).map_err(|_| PdfInspectorProbeFailure {
        code: PdfInspectorProbeFailureCode::InvalidArguments,
        message: "PDF inspector probe arguments invalid: source path must be UTF-8".to_string(),
    })
}

fn run_pdf_inspector_probe(
    source_path: Utf8PathBuf,
) -> Result<PdfInspectorProbeReport, PdfInspectorProbeFailure> {
    let source_bytes = read_bounded_pdf_source(&source_path)?;
    let source_identity = PdfInspectorSourceIdentity {
        blake3: blake3::hash(&source_bytes).to_hex().to_string(),
        byte_len: source_bytes.len() as u64,
    };

    let options = PdfOptions::new()
        .mode(ProcessMode::Analyze)
        .detection(DetectionConfig {
            strategy: ScanStrategy::Full,
            ..DetectionConfig::default()
        });
    let result = pdf_inspector::process_pdf_mem_with_options(&source_bytes, options)
        .map_err(map_pdf_inspector_failure)?;
    if result.page_count > PDF_INSPECTOR_PROBE_MAX_REPORT_PAGES {
        return Err(PdfInspectorProbeFailure {
            code: PdfInspectorProbeFailureCode::ReportLimitExceeded,
            message: format!(
                "PDF inspector probe report limit exceeded: {} pages exceeds the {} page artifact limit",
                result.page_count, PDF_INSPECTOR_PROBE_MAX_REPORT_PAGES
            ),
        });
    }

    Ok(PdfInspectorProbeReport {
        schema_version: PDF_INSPECTOR_PROBE_SCHEMA_VERSION,
        source: source_identity,
        inspector: PdfInspectorProbeFingerprint {
            crate_version: PDF_INSPECTOR_CRATE_VERSION.to_string(),
            source_revision: PDF_INSPECTOR_SOURCE_REVISION.to_string(),
            adapter_version: PDF_INSPECTOR_PROBE_ADAPTER_VERSION,
            profile: PdfInspectorProbeProfile::FullPageLayoutAnalysis,
        },
        inspection: build_pdf_inspector_inspection(result)?,
    })
}

fn read_bounded_pdf_source(source_path: &Utf8Path) -> Result<Vec<u8>, PdfInspectorProbeFailure> {
    let file = std::fs::File::open(source_path.as_std_path()).map_err(|error| {
        PdfInspectorProbeFailure {
            code: PdfInspectorProbeFailureCode::SourceIo,
            message: format!("PDF inspector probe source read failed: {error}"),
        }
    })?;
    let mut source_bytes = Vec::new();
    file.take(PDF_INSPECTOR_PROBE_MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut source_bytes)
        .map_err(|error| PdfInspectorProbeFailure {
            code: PdfInspectorProbeFailureCode::SourceIo,
            message: format!("PDF inspector probe source read failed: {error}"),
        })?;
    if source_bytes.len() as u64 > PDF_INSPECTOR_PROBE_MAX_SOURCE_BYTES {
        return Err(PdfInspectorProbeFailure {
            code: PdfInspectorProbeFailureCode::SourceLimitExceeded,
            message: format!(
                "PDF inspector probe source limit exceeded: source is larger than {} bytes",
                PDF_INSPECTOR_PROBE_MAX_SOURCE_BYTES
            ),
        });
    }
    Ok(source_bytes)
}

fn build_pdf_inspector_inspection(
    result: PdfProcessResult,
) -> Result<PdfInspectorInspection, PdfInspectorProbeFailure> {
    let page_count = result.page_count;
    let table_pages = result
        .layout
        .pages_with_tables
        .into_iter()
        .collect::<BTreeSet<_>>();
    let column_pages = result
        .layout
        .pages_with_columns
        .into_iter()
        .collect::<BTreeSet<_>>();
    let ocr_pages = result
        .pages_needing_ocr
        .into_iter()
        .collect::<BTreeSet<_>>();
    let ocr_reasons = result
        .ocr_reasons_by_page
        .into_iter()
        .map(|evidence| (evidence.page, evidence.reasons))
        .collect::<BTreeMap<_, _>>();
    validate_pdf_inspector_evidence(
        page_count,
        result.confidence,
        &table_pages,
        &column_pages,
        &ocr_pages,
        &ocr_reasons,
    )?;
    let pages = build_pdf_inspector_page_evidence(
        page_count,
        &table_pages,
        &column_pages,
        &ocr_pages,
        &ocr_reasons,
    );
    let ocr_page_count = pages
        .iter()
        .filter(|page| matches!(page.recommended_route, PdfInspectorPageRoute::Ocr))
        .count();
    let recommended_route = document_route_for_ocr_pages(page_count, ocr_page_count);

    Ok(PdfInspectorInspection {
        pdf_type: map_pdf_inspector_document_type(result.pdf_type),
        page_count,
        detector_confidence: result.confidence,
        processing_time_ms: result.processing_time_ms,
        has_encoding_issues: result.has_encoding_issues,
        has_complex_layout: result.layout.is_complex,
        recommended_route,
        pages,
    })
}

fn validate_pdf_inspector_evidence(
    page_count: u32,
    detector_confidence: f32,
    table_pages: &BTreeSet<u32>,
    column_pages: &BTreeSet<u32>,
    ocr_pages: &BTreeSet<u32>,
    ocr_reasons: &BTreeMap<u32, Vec<String>>,
) -> Result<(), PdfInspectorProbeFailure> {
    if !detector_confidence.is_finite() || !(0.0..=1.0).contains(&detector_confidence) {
        return Err(invalid_pdf_inspector_output(
            "detector confidence must be finite and within 0.0 through 1.0",
        ));
    }
    let page_in_bounds = |page: &u32| *page >= 1 && *page <= page_count;
    if table_pages.iter().any(|page| !page_in_bounds(page))
        || column_pages.iter().any(|page| !page_in_bounds(page))
        || ocr_pages.iter().any(|page| !page_in_bounds(page))
        || ocr_reasons.keys().any(|page| !page_in_bounds(page))
    {
        return Err(invalid_pdf_inspector_output(
            "page evidence must use one-based page numbers within the parsed page count",
        ));
    }
    if ocr_pages.iter().any(|page| !ocr_reasons.contains_key(page))
        || ocr_reasons.keys().any(|page| !ocr_pages.contains(page))
    {
        return Err(invalid_pdf_inspector_output(
            "OCR page directives and machine-readable reason records must have identical page sets",
        ));
    }
    Ok(())
}

fn invalid_pdf_inspector_output(message: &str) -> PdfInspectorProbeFailure {
    PdfInspectorProbeFailure {
        code: PdfInspectorProbeFailureCode::InvalidInspectorOutput,
        message: format!("PDF inspector probe output invalid: {message}"),
    }
}

fn build_pdf_inspector_page_evidence(
    page_count: u32,
    table_pages: &BTreeSet<u32>,
    column_pages: &BTreeSet<u32>,
    ocr_pages: &BTreeSet<u32>,
    ocr_reasons: &BTreeMap<u32, Vec<String>>,
) -> Vec<PdfInspectorPageEvidence> {
    (1..=page_count)
        .map(|page_number| {
            let needs_ocr = ocr_pages.contains(&page_number);
            let recommended_route = if needs_ocr {
                PdfInspectorPageRoute::Ocr
            } else {
                PdfInspectorPageRoute::NativeExtract
            };
            PdfInspectorPageEvidence {
                page_number,
                has_table_geometry: table_pages.contains(&page_number),
                has_multiple_columns: column_pages.contains(&page_number),
                needs_ocr,
                ocr_reasons: ocr_reasons.get(&page_number).cloned().unwrap_or_default(),
                recommended_route,
            }
        })
        .collect()
}

fn document_route_for_ocr_pages(
    page_count: u32,
    ocr_page_count: usize,
) -> PdfInspectorDocumentRoute {
    if ocr_page_count == 0 {
        PdfInspectorDocumentRoute::NativeExtract
    } else if ocr_page_count == page_count as usize {
        PdfInspectorDocumentRoute::Ocr
    } else {
        PdfInspectorDocumentRoute::MixedPageRouting
    }
}

fn map_pdf_inspector_document_type(pdf_type: PdfType) -> PdfInspectorDocumentType {
    match pdf_type {
        PdfType::TextBased => PdfInspectorDocumentType::TextBased,
        PdfType::Scanned => PdfInspectorDocumentType::Scanned,
        PdfType::ImageBased => PdfInspectorDocumentType::ImageBased,
        PdfType::Mixed => PdfInspectorDocumentType::Mixed,
    }
}

fn map_pdf_inspector_failure(error: PdfError) -> PdfInspectorProbeFailure {
    let code = match error {
        PdfError::Io(_) => PdfInspectorProbeFailureCode::SourceIo,
        PdfError::NotAPdf(_) => PdfInspectorProbeFailureCode::NotPdf,
        PdfError::Encrypted => PdfInspectorProbeFailureCode::EncryptedPdf,
        PdfError::InvalidStructure => PdfInspectorProbeFailureCode::InvalidPdfStructure,
        PdfError::Parse(_) => PdfInspectorProbeFailureCode::PdfParse,
    };
    PdfInspectorProbeFailure {
        code,
        message: format!("PDF inspector probe analysis failed: {error}"),
    }
}

fn write_probe_envelope(envelope: &PdfInspectorProbeEnvelope) -> Result<(), std::io::Error> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer(&mut writer, envelope).map_err(std::io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_evidence_is_complete_ordered_and_conservative() {
        let table_pages = BTreeSet::from([2]);
        let column_pages = BTreeSet::from([1]);
        let ocr_pages = BTreeSet::from([2]);
        let ocr_reasons = BTreeMap::from([(2, vec!["scanned".to_string()])]);

        let pages = build_pdf_inspector_page_evidence(
            3,
            &table_pages,
            &column_pages,
            &ocr_pages,
            &ocr_reasons,
        );

        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].page_number, 1);
        assert!(pages[0].has_multiple_columns);
        assert!(pages[1].has_table_geometry);
        assert!(pages[1].needs_ocr);
        assert!(matches!(
            pages[1].recommended_route,
            PdfInspectorPageRoute::Ocr
        ));
        assert!(matches!(
            pages[2].recommended_route,
            PdfInspectorPageRoute::NativeExtract
        ));
    }

    #[test]
    fn inspector_evidence_rejects_invalid_pages_and_incomplete_ocr_reasons() {
        let no_pages = BTreeSet::new();
        let out_of_range_pages = BTreeSet::from([3]);
        let missing_reasons_page = BTreeSet::from([1]);

        assert!(validate_pdf_inspector_evidence(
            2,
            0.9,
            &out_of_range_pages,
            &no_pages,
            &no_pages,
            &BTreeMap::new(),
        )
        .is_err());
        assert!(validate_pdf_inspector_evidence(
            2,
            0.9,
            &no_pages,
            &no_pages,
            &missing_reasons_page,
            &BTreeMap::new(),
        )
        .is_err());
        assert!(validate_pdf_inspector_evidence(
            2,
            f32::NAN,
            &no_pages,
            &no_pages,
            &no_pages,
            &BTreeMap::new(),
        )
        .is_err());
    }

    #[test]
    fn document_route_distinguishes_native_ocr_and_mixed_work() {
        assert_eq!(
            document_route_for_ocr_pages(3, 0),
            PdfInspectorDocumentRoute::NativeExtract
        );
        assert_eq!(
            document_route_for_ocr_pages(3, 3),
            PdfInspectorDocumentRoute::Ocr
        );
        assert_eq!(
            document_route_for_ocr_pages(3, 1),
            PdfInspectorDocumentRoute::MixedPageRouting
        );
    }

    #[test]
    fn success_protocol_is_a_discriminated_union() {
        let envelope = PdfInspectorProbeEnvelope::Success {
            report: PdfInspectorProbeReport {
                schema_version: PDF_INSPECTOR_PROBE_SCHEMA_VERSION,
                source: PdfInspectorSourceIdentity {
                    blake3: "abc".to_string(),
                    byte_len: 3,
                },
                inspector: PdfInspectorProbeFingerprint {
                    crate_version: PDF_INSPECTOR_CRATE_VERSION.to_string(),
                    source_revision: PDF_INSPECTOR_SOURCE_REVISION.to_string(),
                    adapter_version: PDF_INSPECTOR_PROBE_ADAPTER_VERSION,
                    profile: PdfInspectorProbeProfile::FullPageLayoutAnalysis,
                },
                inspection: PdfInspectorInspection {
                    pdf_type: PdfInspectorDocumentType::TextBased,
                    page_count: 0,
                    detector_confidence: 1.0,
                    processing_time_ms: 0,
                    has_encoding_issues: false,
                    has_complex_layout: false,
                    recommended_route: PdfInspectorDocumentRoute::NativeExtract,
                    pages: Vec::new(),
                },
            },
        };

        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(value["status"], "success");
        assert!(value.get("error").is_none());
        assert_eq!(value["report"]["schema_version"], 1);
    }
}
