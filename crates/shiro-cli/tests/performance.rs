//! Performance smoke tests for shiro CLI.
//!
//! These are NOT benchmarks — they establish reasonable latency bounds
//! and report P50/P95 timings as tracing output. They exist to catch
//! catastrophic performance regressions early.

use std::process::Command;
use std::time::Instant;

/// Run the shiro binary with the given args, using a temporary home directory.
fn shiro(home: &std::path::Path, args: &[&str]) -> (String, i32) {
    let bin = env!("CARGO_BIN_EXE_shiro");
    let output = Command::new(bin)
        .args(["--home", home.to_str().unwrap()])
        .args(["--log-level", "silent"])
        .args(args)
        .output()
        .expect("failed to execute shiro binary");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, code)
}

fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_else(|e| {
        panic!("failed to parse JSON: {e}\nstdout: {stdout}");
    })
}

/// Compute percentile from a sorted slice. `p` is in [0, 100].
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
fn test_ingest_latency_smoke() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-perf-ingest");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    // Create 20 small text files.
    let corpus = tmp.path().join("corpus");
    std::fs::create_dir(&corpus).unwrap();
    for i in 0..20 {
        let content = format!(
            "Performance test document number {i}.\n\n\
             This document contains enough text to exercise the parser, \
             segmenter, and indexing pipeline. Topic: {topic}.",
            i = i,
            topic = [
                "algorithms",
                "databases",
                "networking",
                "compilers",
                "security"
            ][i % 5]
        );
        std::fs::write(corpus.join(format!("doc_{i:03}.txt")), content).unwrap();
    }

    // Measure ingest latency.
    let start = Instant::now();
    let (stdout, code) = shiro(&home, &["ingest", corpus.to_str().unwrap()]);
    let elapsed = start.elapsed();

    assert_eq!(code, 0, "ingest failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());

    let elapsed_ms = elapsed.as_millis();
    eprintln!(
        "[perf] ingest 20 files: {}ms ({:.1}ms/doc)",
        elapsed_ms,
        elapsed_ms as f64 / 20.0
    );

    // Smoke bound: 20 files should complete in under 5 seconds.
    assert!(
        elapsed.as_secs() < 5,
        "ingest 20 files took {elapsed_ms}ms — exceeds 5s smoke bound"
    );
}

#[test]
fn test_search_latency_smoke() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-perf-search");

    // Setup: init + ingest files.
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let corpus = tmp.path().join("search-corpus");
    std::fs::create_dir(&corpus).unwrap();
    for i in 0..20 {
        let content = format!(
            "Document {i} about {topic}.\n\n\
             {topic} is an important field in computer science. \
             Understanding {topic} helps build better systems.",
            i = i,
            topic = [
                "algorithms",
                "databases",
                "networking",
                "compilers",
                "security",
                "machine learning",
                "distributed systems",
                "operating systems",
                "programming languages",
                "software engineering"
            ][i % 10]
        );
        std::fs::write(corpus.join(format!("search_{i:03}.txt")), content).unwrap();
    }

    let (stdout, code) = shiro(&home, &["ingest", corpus.to_str().unwrap()]);
    assert_eq!(code, 0, "ingest failed: {stdout}");

    // Measure search latency over 50 queries.
    let queries = [
        "algorithms",
        "database systems",
        "network security",
        "compiler optimization",
        "machine learning",
        "distributed computing",
        "operating system kernel",
        "programming language design",
        "software architecture",
        "data structures",
    ];

    let mut latencies_ms: Vec<f64> = Vec::with_capacity(50);

    for i in 0..50 {
        let query = queries[i % queries.len()];
        let start = Instant::now();
        let (stdout, code) = shiro(&home, &["search", query]);
        let elapsed = start.elapsed();
        assert_eq!(code, 0, "search '{query}' failed: {stdout}");
        latencies_ms.push(elapsed.as_secs_f64() * 1000.0);
    }

    latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p50 = percentile(&latencies_ms, 50.0);
    let p95 = percentile(&latencies_ms, 95.0);
    let p99 = percentile(&latencies_ms, 99.0);
    let min = latencies_ms.first().copied().unwrap_or(0.0);
    let max = latencies_ms.last().copied().unwrap_or(0.0);

    eprintln!(
        "[perf] search latency (50 queries, 20 docs): \
         min={min:.1}ms P50={p50:.1}ms P95={p95:.1}ms P99={p99:.1}ms max={max:.1}ms"
    );

    // Smoke bound: P95 should be under 500ms for a brute-force index on 20 docs.
    assert!(
        p95 < 500.0,
        "search P95 = {p95:.1}ms — exceeds 500ms smoke bound"
    );
}
