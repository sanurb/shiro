//! Judged retrieval benchmark evaluator (ADR-027).
//!
//! The evaluator never invents judgments or hardware claims. It validates a
//! versioned manifest against the authoritative store, scopes every query to the
//! frozen corpus, and reports deterministic relevance, latency, explain, and
//! ranking-integrity evidence. Corpus sufficiency is explicit in the result.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use shiro_core::{DocId, ShiroError};

use crate::Engine;

use super::explain::ExplainInput;
use super::search::{SearchFilters, SearchInput, SearchMode};

pub const BENCHMARK_MANIFEST_VERSION: u32 = 1;
pub const MIN_RELEASE_GATE_DOCUMENTS: usize = 100;
pub const MIN_RELEASE_GATE_QUERIES: usize = 300;
const REBUILD_EVIDENCE_PENDING: &str = "rebuild integrity has not been evaluated";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkManifest {
    pub schema_version: u32,
    pub corpus_id: String,
    pub corpus_version: String,
    pub documents: Vec<BenchmarkDocument>,
    pub queries: Vec<BenchmarkQuery>,
    pub hardware_profile: BenchmarkHardwareProfile,
    pub pipelines: Vec<BenchmarkPipeline>,
    pub thresholds: BenchmarkThresholds,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkDocument {
    pub doc_id: String,
    pub source_uri: String,
    pub source_hash: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkQuery {
    pub query_id: String,
    pub text: String,
    pub judgments: Vec<BenchmarkJudgment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkJudgment {
    pub doc_id: String,
    /// Graded relevance in [0, 3]. Zero is explicitly non-relevant.
    pub relevance: u8,
    /// Canonical evidence locator supplied by corpus adjudicators.
    pub source_locator: String,
    pub assessor_ids: Vec<String>,
    pub adjudicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkHardwareProfile {
    pub profile_id: String,
    pub cpu_model: String,
    pub logical_cores: usize,
    pub memory_bytes: u64,
    pub os: String,
    pub architecture: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkPipeline {
    Bm25,
    Vector,
    Hybrid,
    HybridRerank,
}

impl BenchmarkPipeline {
    fn name(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::Vector => "vector",
            Self::Hybrid => "hybrid",
            Self::HybridRerank => "hybrid_rerank",
        }
    }

    fn search_mode(self) -> SearchMode {
        match self {
            Self::Bm25 => SearchMode::Bm25,
            Self::Vector => SearchMode::Vector,
            Self::Hybrid | Self::HybridRerank => SearchMode::Hybrid,
        }
    }

    fn rerank(self) -> bool {
        matches!(self, Self::HybridRerank)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkThresholds {
    pub min_candidate_recall_at_50: f64,
    pub min_ndcg_at_10: f64,
    pub min_mrr_at_10: f64,
    pub max_search_p95_ms: f64,
    /// Optional paired nDCG improvement gate relative to BM25.
    pub min_paired_ndcg_delta_ci95_lower: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BenchmarkOutput {
    pub schema_version: u32,
    pub corpus_id: String,
    pub corpus_version: String,
    pub manifest_digest: String,
    pub evidence_status: String,
    pub evidence_reasons: Vec<String>,
    pub document_count: usize,
    pub query_count: usize,
    pub hardware: BenchmarkHardwareEvidence,
    pub pipelines: Vec<PipelineBenchmarkResult>,
    pub rebuild_integrity: Option<RebuildIntegrity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BenchmarkHardwareEvidence {
    pub declared: BenchmarkHardwareProfile,
    pub observed_os: String,
    pub observed_architecture: String,
    pub observed_cpu_model: Option<String>,
    pub observed_logical_cores: Option<usize>,
    pub observed_memory_bytes: Option<u64>,
    pub observed_rss_bytes: Option<u64>,
    pub profile_matches_observed_platform: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PipelineBenchmarkResult {
    pub pipeline: String,
    pub status: String,
    pub unavailable_reason: Option<String>,
    pub candidate_recall_limit: usize,
    pub final_rerank_candidate_limit: Option<usize>,
    pub final_result_limit: usize,
    pub candidate_recall_at_50: Option<f64>,
    pub precision_at_10: Option<f64>,
    pub recall_at_10: Option<f64>,
    pub mrr_at_10: Option<f64>,
    pub ndcg_at_10: Option<f64>,
    pub search_latency_ms: Option<LatencyPercentiles>,
    pub explain_complete: Option<bool>,
    pub deterministic_across_runs: Option<bool>,
    pub ranking_digest: Option<String>,
    pub paired_ndcg_delta_vs_bm25: Option<PairedConfidenceInterval>,
    pub thresholds_passed: Option<bool>,
    #[serde(skip)]
    per_query_ndcg: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LatencyPercentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PairedConfidenceInterval {
    pub mean: f64,
    pub ci95_lower: f64,
    pub ci95_upper: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RebuildIntegrity {
    pub passed: bool,
    pub before: BTreeMap<String, String>,
    pub after: BTreeMap<String, String>,
}

impl BenchmarkOutput {
    /// Attach post-rebuild ranking evidence and recompute the gate status.
    pub fn with_rebuild_integrity(mut self, rebuilt: &BenchmarkOutput) -> Self {
        let before = ranking_digests(&self.pipelines);
        let after = ranking_digests(&rebuilt.pipelines);
        let passed = before == after;
        self.rebuild_integrity = Some(RebuildIntegrity {
            passed,
            before,
            after,
        });
        self.evidence_reasons
            .retain(|reason| reason != REBUILD_EVIDENCE_PENDING);
        if !passed {
            self.evidence_status = "fail".to_string();
            self.evidence_reasons
                .push("rankings changed after deterministic rebuild".to_string());
        } else if !self.evidence_reasons.is_empty() {
            self.evidence_status = "insufficient_evidence".to_string();
        } else if self
            .pipelines
            .iter()
            .all(|pipeline| pipeline.thresholds_passed == Some(true))
        {
            self.evidence_status = "pass".to_string();
        } else {
            self.evidence_status = "fail".to_string();
        }
        self
    }
}

pub fn execute(
    engine: &Engine,
    manifest: &BenchmarkManifest,
    warmup_runs: usize,
    measured_runs: usize,
) -> Result<BenchmarkOutput, ShiroError> {
    validate_manifest(engine, manifest)?;
    if measured_runs == 0 {
        return Err(ShiroError::InvalidInput {
            message: "benchmark measured_runs must be at least 1".to_string(),
        });
    }

    let corpus_doc_ids = manifest
        .documents
        .iter()
        .map(|document| document.doc_id.clone())
        .collect::<Vec<_>>();
    let mut pipeline_results = Vec::new();
    for pipeline in &manifest.pipelines {
        pipeline_results.push(evaluate_pipeline(
            engine,
            manifest,
            *pipeline,
            &corpus_doc_ids,
            warmup_runs,
            measured_runs,
        )?);
    }

    let bm25_ndcg = pipeline_results
        .iter()
        .find(|result| result.pipeline == BenchmarkPipeline::Bm25.name())
        .map(|result| result.per_query_ndcg.clone());
    if let Some(baseline) = bm25_ndcg {
        for result in &mut pipeline_results {
            if result.pipeline != BenchmarkPipeline::Bm25.name()
                && result.status == "measured"
                && result.per_query_ndcg.len() == baseline.len()
            {
                let deltas = result
                    .per_query_ndcg
                    .iter()
                    .zip(&baseline)
                    .map(|(candidate, control)| candidate - control)
                    .collect::<Vec<_>>();
                result.paired_ndcg_delta_vs_bm25 = Some(paired_bootstrap_ci(&deltas, 2_000));
            }
        }
    }

    for result in &mut pipeline_results {
        if result.status != "measured" {
            continue;
        }
        let mut passed = result.candidate_recall_at_50.unwrap_or(0.0)
            >= manifest.thresholds.min_candidate_recall_at_50
            && result.ndcg_at_10.unwrap_or(0.0) >= manifest.thresholds.min_ndcg_at_10
            && result.mrr_at_10.unwrap_or(0.0) >= manifest.thresholds.min_mrr_at_10
            && result
                .search_latency_ms
                .as_ref()
                .map(|latency| latency.p95 <= manifest.thresholds.max_search_p95_ms)
                .unwrap_or(false)
            && result.explain_complete == Some(true)
            && result.deterministic_across_runs == Some(true);
        if result.pipeline != BenchmarkPipeline::Bm25.name() {
            if let Some(minimum) = manifest.thresholds.min_paired_ndcg_delta_ci95_lower {
                passed &= result
                    .paired_ndcg_delta_vs_bm25
                    .as_ref()
                    .map(|interval| interval.ci95_lower >= minimum)
                    .unwrap_or(false);
            }
        }
        result.thresholds_passed = Some(passed);
    }

    let observed_os = std::env::consts::OS.to_string();
    let observed_architecture = std::env::consts::ARCH.to_string();
    let observed_cpu_model = observed_cpu_model();
    let observed_logical_cores = std::thread::available_parallelism().ok().map(usize::from);
    let observed_memory_bytes = observed_memory_bytes();
    let platform_matches = manifest
        .hardware_profile
        .os
        .eq_ignore_ascii_case(&observed_os)
        && manifest
            .hardware_profile
            .architecture
            .eq_ignore_ascii_case(&observed_architecture)
        && observed_cpu_model
            .as_deref()
            .is_some_and(|cpu| cpu == manifest.hardware_profile.cpu_model.trim())
        && observed_logical_cores == Some(manifest.hardware_profile.logical_cores)
        && observed_memory_bytes == Some(manifest.hardware_profile.memory_bytes);
    let mut reasons = Vec::new();
    if manifest.documents.len() < MIN_RELEASE_GATE_DOCUMENTS {
        reasons.push(format!(
            "requires at least {MIN_RELEASE_GATE_DOCUMENTS} documents; manifest has {}",
            manifest.documents.len()
        ));
    }
    if manifest.queries.len() < MIN_RELEASE_GATE_QUERIES {
        reasons.push(format!(
            "requires at least {MIN_RELEASE_GATE_QUERIES} queries; manifest has {}",
            manifest.queries.len()
        ));
    }
    if !platform_matches {
        reasons.push(
            "declared hardware profile does not match observed CPU/memory/platform".to_string(),
        );
    }
    let all_measured = pipeline_results
        .iter()
        .all(|result| result.status == "measured");
    if !all_measured {
        reasons.push("one or more requested pipelines were unavailable".to_string());
    }
    let all_thresholds_passed = pipeline_results
        .iter()
        .all(|result| result.thresholds_passed == Some(true));
    reasons.push(REBUILD_EVIDENCE_PENDING.to_string());
    let evidence_status = if !reasons.is_empty() {
        "insufficient_evidence"
    } else if all_thresholds_passed {
        "pass"
    } else {
        "fail"
    };

    Ok(BenchmarkOutput {
        schema_version: BENCHMARK_MANIFEST_VERSION,
        corpus_id: manifest.corpus_id.clone(),
        corpus_version: manifest.corpus_version.clone(),
        manifest_digest: blake3::hash(&serde_json::to_vec(manifest).map_err(|error| {
            ShiroError::InvalidInput {
                message: format!("failed to canonicalize benchmark manifest: {error}"),
            }
        })?)
        .to_hex()
        .to_string(),
        evidence_status: evidence_status.to_string(),
        evidence_reasons: reasons,
        document_count: manifest.documents.len(),
        query_count: manifest.queries.len(),
        hardware: BenchmarkHardwareEvidence {
            declared: manifest.hardware_profile.clone(),
            observed_os,
            observed_architecture,
            observed_cpu_model,
            observed_logical_cores,
            observed_memory_bytes,
            observed_rss_bytes: observed_rss_bytes(),
            profile_matches_observed_platform: platform_matches,
        },
        pipelines: pipeline_results,
        rebuild_integrity: None,
    })
}

fn validate_manifest(engine: &Engine, manifest: &BenchmarkManifest) -> Result<(), ShiroError> {
    if manifest.schema_version != BENCHMARK_MANIFEST_VERSION {
        return Err(ShiroError::InvalidInput {
            message: format!(
                "unsupported benchmark manifest version {}; expected {BENCHMARK_MANIFEST_VERSION}",
                manifest.schema_version
            ),
        });
    }
    if manifest.corpus_id.trim().is_empty() || manifest.corpus_version.trim().is_empty() {
        return Err(ShiroError::InvalidInput {
            message: "benchmark corpus_id and corpus_version must be non-empty".to_string(),
        });
    }
    if manifest.documents.is_empty() || manifest.queries.is_empty() {
        return Err(ShiroError::InvalidInput {
            message: "benchmark manifest requires documents and queries".to_string(),
        });
    }
    if manifest.pipelines.is_empty() {
        return Err(ShiroError::InvalidInput {
            message: "benchmark manifest requires at least one pipeline".to_string(),
        });
    }
    validate_thresholds(&manifest.thresholds)?;

    let mut document_ids = HashSet::new();
    for expected in &manifest.documents {
        if expected.source_uri.trim().is_empty()
            || expected.source_hash.trim().is_empty()
            || expected.license.trim().is_empty()
        {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "benchmark document {} requires source_uri, source_hash, and license",
                    expected.doc_id
                ),
            });
        }
        if !document_ids.insert(expected.doc_id.as_str()) {
            return Err(ShiroError::InvalidInput {
                message: format!("duplicate benchmark document: {}", expected.doc_id),
            });
        }
        let doc_id =
            DocId::from_stored(&expected.doc_id).map_err(|error| ShiroError::InvalidInput {
                message: format!("invalid benchmark document ID: {error}"),
            })?;
        let (stored, state) = engine.store.get_document(&doc_id)?;
        if state.as_str() != "READY" {
            return Err(ShiroError::InvalidInput {
                message: format!("benchmark document {doc_id} is not READY"),
            });
        }
        if stored.metadata.source_hash != expected.source_hash
            || stored.metadata.source_uri != expected.source_uri
        {
            return Err(ShiroError::InvalidInput {
                message: format!("benchmark source identity mismatch for {doc_id}"),
            });
        }
    }

    let mut query_ids = HashSet::new();
    for query in &manifest.queries {
        if query.query_id.trim().is_empty()
            || query.text.trim().is_empty()
            || !query_ids.insert(query.query_id.as_str())
        {
            return Err(ShiroError::InvalidInput {
                message: format!("invalid or duplicate benchmark query: {}", query.query_id),
            });
        }
        if query.judgments.is_empty() {
            return Err(ShiroError::InvalidInput {
                message: format!("benchmark query {} has no judgments", query.query_id),
            });
        }
        let mut has_relevant = false;
        let mut judged_documents = HashSet::new();
        for judgment in &query.judgments {
            if judgment.relevance > 3
                || judgment.source_locator.trim().is_empty()
                || judgment.assessor_ids.is_empty()
                || judgment
                    .assessor_ids
                    .iter()
                    .any(|assessor| assessor.trim().is_empty())
                || !judgment.adjudicated
            {
                return Err(ShiroError::InvalidInput {
                    message: format!(
                        "query {} has an invalid or unadjudicated judgment for {}",
                        query.query_id, judgment.doc_id
                    ),
                });
            }
            if !document_ids.contains(judgment.doc_id.as_str())
                || !judged_documents.insert(judgment.doc_id.as_str())
            {
                return Err(ShiroError::InvalidInput {
                    message: format!(
                        "query {} judgment references an unknown or duplicate document {}",
                        query.query_id, judgment.doc_id
                    ),
                });
            }
            has_relevant |= judgment.relevance > 0;
        }
        if !has_relevant {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "benchmark query {} has no relevant judgment",
                    query.query_id
                ),
            });
        }
    }
    Ok(())
}

fn validate_thresholds(thresholds: &BenchmarkThresholds) -> Result<(), ShiroError> {
    for (name, value) in [
        (
            "min_candidate_recall_at_50",
            thresholds.min_candidate_recall_at_50,
        ),
        ("min_ndcg_at_10", thresholds.min_ndcg_at_10),
        ("min_mrr_at_10", thresholds.min_mrr_at_10),
    ] {
        if !(0.0..=1.0).contains(&value) || !value.is_finite() {
            return Err(ShiroError::InvalidInput {
                message: format!("benchmark threshold {name} must be finite and in [0, 1]"),
            });
        }
    }
    if thresholds.max_search_p95_ms <= 0.0 || !thresholds.max_search_p95_ms.is_finite() {
        return Err(ShiroError::InvalidInput {
            message: "max_search_p95_ms must be finite and positive".to_string(),
        });
    }
    if thresholds
        .min_paired_ndcg_delta_ci95_lower
        .is_some_and(|value| !value.is_finite() || !(-1.0..=1.0).contains(&value))
    {
        return Err(ShiroError::InvalidInput {
            message: "paired nDCG threshold must be finite and in [-1, 1]".to_string(),
        });
    }
    Ok(())
}

fn evaluate_pipeline(
    engine: &Engine,
    manifest: &BenchmarkManifest,
    pipeline: BenchmarkPipeline,
    corpus_doc_ids: &[String],
    warmup_runs: usize,
    measured_runs: usize,
) -> Result<PipelineBenchmarkResult, ShiroError> {
    let filters = SearchFilters {
        document_ids: corpus_doc_ids.to_vec(),
        ..SearchFilters::default()
    };
    for _ in 0..warmup_runs {
        for query in &manifest.queries {
            let input = benchmark_search_input(query.text.clone(), pipeline, filters.clone(), 10);
            match engine.search(&input) {
                Ok(output) => {
                    if let Some(reason) = inactive_pipeline_reason(pipeline, &output) {
                        return Ok(unavailable_pipeline_reason(pipeline, reason));
                    }
                }
                Err(error) => return Ok(unavailable_pipeline(pipeline, error)),
            }
        }
    }

    let mut latencies = Vec::with_capacity(manifest.queries.len() * measured_runs);
    let mut ranking_runs: Vec<Vec<Vec<String>>> = Vec::with_capacity(measured_runs);
    let mut metric_accumulator = MetricAccumulator::default();
    let mut explain_complete = true;

    for run in 0..measured_runs {
        let mut run_rankings = Vec::with_capacity(manifest.queries.len());
        for query in &manifest.queries {
            let input = benchmark_search_input(query.text.clone(), pipeline, filters.clone(), 10);
            let started = Instant::now();
            let output = match engine.search(&input) {
                Ok(output) => output,
                Err(error) => return Ok(unavailable_pipeline(pipeline, error)),
            };
            latencies.push(started.elapsed().as_secs_f64() * 1_000.0);
            if let Some(reason) = inactive_pipeline_reason(pipeline, &output) {
                return Ok(unavailable_pipeline_reason(pipeline, reason));
            }
            let ranking =
                unique_document_ranking(output.hits.iter().map(|hit| hit.doc_id.as_str()));
            if run == 0 {
                let grades = judgment_grades(query);
                metric_accumulator.add_final(&ranking, &grades);
                for hit in &output.hits {
                    explain_complete &= engine
                        .explain(&ExplainInput {
                            result_id: hit.result_id.clone(),
                        })
                        .map(|explain| !explain.retrieval_trace.stages.is_empty())
                        .unwrap_or(false);
                }
            }
            run_rankings.push(ranking);
        }
        ranking_runs.push(run_rankings);
    }

    for query in &manifest.queries {
        let input = benchmark_search_input(query.text.clone(), pipeline, filters.clone(), 50);
        let output = match engine.search(&input) {
            Ok(output) => output,
            Err(error) => return Ok(unavailable_pipeline(pipeline, error)),
        };
        if let Some(reason) = inactive_pipeline_reason(pipeline, &output) {
            return Ok(unavailable_pipeline_reason(pipeline, reason));
        }
        let ranking = unique_document_ranking(output.hits.iter().map(|hit| hit.doc_id.as_str()));
        metric_accumulator.add_candidate(&ranking, &judgment_grades(query));
    }

    let deterministic = ranking_runs
        .first()
        .map(|first| ranking_runs.iter().all(|ranking| ranking == first))
        .unwrap_or(true);
    let ranking_digest = ranking_runs.first().map(|rankings| {
        let mut hasher = blake3::Hasher::new();
        for (query, ranking) in manifest.queries.iter().zip(rankings) {
            hasher.update(query.query_id.as_bytes());
            hasher.update(b"\0");
            for doc_id in ranking {
                hasher.update(doc_id.as_bytes());
                hasher.update(b"\0");
            }
        }
        hasher.finalize().to_hex().to_string()
    });
    let query_count = manifest.queries.len() as f64;
    latencies.sort_by(f64::total_cmp);

    Ok(PipelineBenchmarkResult {
        pipeline: pipeline.name().to_string(),
        status: "measured".to_string(),
        unavailable_reason: None,
        candidate_recall_limit: 50,
        final_rerank_candidate_limit: if pipeline.rerank() {
            engine
                .reranker()
                .map(|reranker| reranker.rerank_candidate_limit().candidate_count())
        } else {
            None
        },
        final_result_limit: 10,
        candidate_recall_at_50: Some(metric_accumulator.candidate_recall / query_count),
        precision_at_10: Some(metric_accumulator.precision / query_count),
        recall_at_10: Some(metric_accumulator.recall / query_count),
        mrr_at_10: Some(metric_accumulator.mrr / query_count),
        ndcg_at_10: Some(metric_accumulator.ndcg / query_count),
        search_latency_ms: Some(LatencyPercentiles {
            p50: percentile(&latencies, 0.50),
            p95: percentile(&latencies, 0.95),
            p99: percentile(&latencies, 0.99),
            samples: latencies.len(),
        }),
        explain_complete: Some(explain_complete),
        deterministic_across_runs: Some(deterministic),
        ranking_digest,
        paired_ndcg_delta_vs_bm25: None,
        thresholds_passed: None,
        per_query_ndcg: metric_accumulator.per_query_ndcg,
    })
}

fn unique_document_ranking<'a>(doc_ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    doc_ids
        .filter(|doc_id| seen.insert(*doc_id))
        .map(str::to_string)
        .collect()
}

fn benchmark_search_input(
    query: String,
    pipeline: BenchmarkPipeline,
    filters: SearchFilters,
    limit: usize,
) -> SearchInput {
    SearchInput {
        query,
        mode: pipeline.search_mode(),
        limit,
        expand: false,
        max_blocks: 12,
        max_chars: 8_000,
        rerank: pipeline.rerank(),
        filters,
    }
}

fn inactive_pipeline_reason(
    pipeline: BenchmarkPipeline,
    output: &super::search::SearchOutput,
) -> Option<String> {
    if !matches!(pipeline, BenchmarkPipeline::Bm25) && !output.retrieval_info.vector_active {
        return Some("requested vector source was not active".to_string());
    }
    if pipeline.rerank() && !output.retrieval_info.reranker_active {
        return Some("requested reranker was not active".to_string());
    }
    None
}

fn unavailable_pipeline(pipeline: BenchmarkPipeline, error: ShiroError) -> PipelineBenchmarkResult {
    unavailable_pipeline_reason(pipeline, error.to_string())
}

fn unavailable_pipeline_reason(
    pipeline: BenchmarkPipeline,
    reason: String,
) -> PipelineBenchmarkResult {
    PipelineBenchmarkResult {
        pipeline: pipeline.name().to_string(),
        status: "unavailable".to_string(),
        unavailable_reason: Some(reason),
        candidate_recall_limit: 50,
        final_rerank_candidate_limit: None,
        final_result_limit: 10,
        candidate_recall_at_50: None,
        precision_at_10: None,
        recall_at_10: None,
        mrr_at_10: None,
        ndcg_at_10: None,
        search_latency_ms: None,
        explain_complete: None,
        deterministic_across_runs: None,
        ranking_digest: None,
        paired_ndcg_delta_vs_bm25: None,
        thresholds_passed: None,
        per_query_ndcg: Vec::new(),
    }
}

#[derive(Default)]
struct MetricAccumulator {
    candidate_recall: f64,
    precision: f64,
    recall: f64,
    mrr: f64,
    ndcg: f64,
    per_query_ndcg: Vec<f64>,
}

impl MetricAccumulator {
    fn add_candidate(&mut self, ranking: &[String], grades: &HashMap<&str, u8>) {
        self.candidate_recall += recall_at(ranking, grades, 50);
    }

    fn add_final(&mut self, ranking: &[String], grades: &HashMap<&str, u8>) {
        self.precision += precision_at(ranking, grades, 10);
        self.recall += recall_at(ranking, grades, 10);
        self.mrr += reciprocal_rank_at(ranking, grades, 10);
        let ndcg = ndcg_at(ranking, grades, 10);
        self.ndcg += ndcg;
        self.per_query_ndcg.push(ndcg);
    }
}

fn judgment_grades(query: &BenchmarkQuery) -> HashMap<&str, u8> {
    query
        .judgments
        .iter()
        .map(|judgment| (judgment.doc_id.as_str(), judgment.relevance))
        .collect()
}

fn precision_at(ranking: &[String], grades: &HashMap<&str, u8>, k: usize) -> f64 {
    let relevant = ranking
        .iter()
        .take(k)
        .filter(|doc_id| grades.get(doc_id.as_str()).copied().unwrap_or(0) > 0)
        .count();
    relevant as f64 / k as f64
}

fn recall_at(ranking: &[String], grades: &HashMap<&str, u8>, k: usize) -> f64 {
    let relevant_total = grades.values().filter(|grade| **grade > 0).count();
    if relevant_total == 0 {
        return 0.0;
    }
    let retrieved = ranking
        .iter()
        .take(k)
        .filter(|doc_id| grades.get(doc_id.as_str()).copied().unwrap_or(0) > 0)
        .collect::<HashSet<_>>()
        .len();
    retrieved as f64 / relevant_total as f64
}

fn reciprocal_rank_at(ranking: &[String], grades: &HashMap<&str, u8>, k: usize) -> f64 {
    ranking
        .iter()
        .take(k)
        .position(|doc_id| grades.get(doc_id.as_str()).copied().unwrap_or(0) > 0)
        .map(|index| 1.0 / (index + 1) as f64)
        .unwrap_or(0.0)
}

fn ndcg_at(ranking: &[String], grades: &HashMap<&str, u8>, k: usize) -> f64 {
    let dcg = ranking
        .iter()
        .take(k)
        .enumerate()
        .map(|(index, doc_id)| {
            discounted_gain(grades.get(doc_id.as_str()).copied().unwrap_or(0), index)
        })
        .sum::<f64>();
    let mut ideal = grades.values().copied().collect::<Vec<_>>();
    ideal.sort_unstable_by(|left, right| right.cmp(left));
    let idcg = ideal
        .into_iter()
        .take(k)
        .enumerate()
        .map(|(index, grade)| discounted_gain(grade, index))
        .sum::<f64>();
    if idcg == 0.0 {
        0.0
    } else {
        dcg / idcg
    }
}

fn discounted_gain(grade: u8, zero_based_rank: usize) -> f64 {
    let gain = 2_f64.powi(i32::from(grade)) - 1.0;
    gain / ((zero_based_rank + 2) as f64).log2()
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = (percentile.clamp(0.0, 1.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[index]
}

fn paired_bootstrap_ci(deltas: &[f64], iterations: usize) -> PairedConfidenceInterval {
    if deltas.is_empty() {
        return PairedConfidenceInterval {
            mean: 0.0,
            ci95_lower: 0.0,
            ci95_upper: 0.0,
            samples: 0,
        };
    }
    let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let mut state = 0x5eed_cafe_f00d_u64;
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let mut sample_sum = 0.0;
        for _ in 0..deltas.len() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            sample_sum += deltas[(state as usize) % deltas.len()];
        }
        samples.push(sample_sum / deltas.len() as f64);
    }
    samples.sort_by(f64::total_cmp);
    PairedConfidenceInterval {
        mean,
        ci95_lower: percentile(&samples, 0.025),
        ci95_upper: percentile(&samples, 0.975),
        samples: deltas.len(),
    }
}

fn ranking_digests(pipelines: &[PipelineBenchmarkResult]) -> BTreeMap<String, String> {
    pipelines
        .iter()
        .filter_map(|pipeline| {
            pipeline
                .ranking_digest
                .as_ref()
                .map(|digest| (pipeline.pipeline.clone(), digest.clone()))
        })
        .collect()
}

fn observed_cpu_model() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        cpuinfo.lines().find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|value| value.split_once(':'))
                .map(|(_, value)| value.trim().to_string())
        })
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()?;
        Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

fn observed_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kilobytes = meminfo
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        Some(kilobytes.saturating_mul(1_024))
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

fn observed_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let kilobytes = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        Some(kilobytes.saturating_mul(1_024))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        let kilobytes = String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?;
        Some(kilobytes.saturating_mul(1_024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grades() -> HashMap<&'static str, u8> {
        HashMap::from([("doc_a", 3), ("doc_b", 1), ("doc_c", 0)])
    }

    #[test]
    fn benchmark_manifest_template_matches_typed_contract() {
        let manifest: BenchmarkManifest = serde_json::from_str(include_str!(
            "../../../../benchmarks/manifest.template.json"
        ))
        .unwrap();

        assert_eq!(manifest.schema_version, BENCHMARK_MANIFEST_VERSION);
        assert_eq!(manifest.documents.len(), 1);
        assert_eq!(manifest.queries.len(), 1);
        assert_eq!(manifest.pipelines.len(), 4);
    }

    #[test]
    fn benchmark_evaluates_frozen_ready_corpus_without_claiming_small_fixture_is_sufficient() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temporary.path().join("home")).unwrap();
        let home = shiro_core::ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let ingested = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &shiro_parse::PlainTextParser,
            "benchmark-fixture.txt",
            b"judged benchmark retrieval evidence",
        )
        .unwrap();
        let (document, _) = engine.store.get_document(&ingested.doc_id).unwrap();
        let manifest = BenchmarkManifest {
            schema_version: BENCHMARK_MANIFEST_VERSION,
            corpus_id: "fixture".to_string(),
            corpus_version: "1".to_string(),
            documents: vec![BenchmarkDocument {
                doc_id: ingested.doc_id.as_str().to_string(),
                source_uri: document.metadata.source_uri,
                source_hash: document.metadata.source_hash,
                license: "test-only".to_string(),
            }],
            queries: vec![BenchmarkQuery {
                query_id: "q1".to_string(),
                text: "retrieval evidence".to_string(),
                judgments: vec![BenchmarkJudgment {
                    doc_id: ingested.doc_id.as_str().to_string(),
                    relevance: 3,
                    source_locator: "bytes:0-35".to_string(),
                    assessor_ids: vec!["test-assessor".to_string()],
                    adjudicated: true,
                }],
            }],
            hardware_profile: BenchmarkHardwareProfile {
                profile_id: "test".to_string(),
                cpu_model: "test".to_string(),
                logical_cores: 1,
                memory_bytes: 1,
                os: std::env::consts::OS.to_string(),
                architecture: std::env::consts::ARCH.to_string(),
            },
            pipelines: vec![BenchmarkPipeline::Bm25],
            thresholds: BenchmarkThresholds {
                min_candidate_recall_at_50: 1.0,
                min_ndcg_at_10: 1.0,
                min_mrr_at_10: 1.0,
                max_search_p95_ms: 10_000.0,
                min_paired_ndcg_delta_ci95_lower: None,
            },
        };

        let output = execute(&engine, &manifest, 0, 2).unwrap();
        assert_eq!(output.evidence_status, "insufficient_evidence");
        assert_eq!(output.pipelines[0].candidate_recall_at_50, Some(1.0));
        assert_eq!(output.pipelines[0].ndcg_at_10, Some(1.0));
        assert_eq!(output.pipelines[0].deterministic_across_runs, Some(true));
        assert_eq!(output.pipelines[0].thresholds_passed, Some(true));

        let mut unavailable = manifest;
        unavailable.pipelines = vec![BenchmarkPipeline::HybridRerank];
        let output = execute(&engine, &unavailable, 0, 1).unwrap();
        assert_eq!(output.pipelines[0].status, "unavailable");
        assert!(output.pipelines[0].unavailable_reason.is_some());
    }

    #[test]
    fn relevance_metrics_use_standard_rank_cutoffs() {
        let ranking = vec![
            "doc_b".to_string(),
            "doc_x".to_string(),
            "doc_a".to_string(),
        ];
        let grades = grades();
        assert!((precision_at(&ranking, &grades, 10) - 0.2).abs() < f64::EPSILON);
        assert!((recall_at(&ranking, &grades, 10) - 1.0).abs() < f64::EPSILON);
        assert!((reciprocal_rank_at(&ranking, &grades, 10) - 1.0).abs() < f64::EPSILON);
        assert!(ndcg_at(&ranking, &grades, 10) > 0.5);
        assert!(ndcg_at(&ranking, &grades, 10) < 1.0);
    }

    #[test]
    fn paired_bootstrap_is_deterministic() {
        let deltas = [0.1, 0.2, -0.1, 0.3];
        let first = paired_bootstrap_ci(&deltas, 500);
        let second = paired_bootstrap_ci(&deltas, 500);
        assert_eq!(first.mean, second.mean);
        assert_eq!(first.ci95_lower, second.ci95_lower);
        assert_eq!(first.ci95_upper, second.ci95_upper);
        assert!(first.ci95_lower <= first.mean);
        assert!(first.ci95_upper >= first.mean);
    }

    #[test]
    fn percentile_handles_boundaries() {
        let values = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 1.0), 4.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }
}
