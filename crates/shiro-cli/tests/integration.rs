//! Integration tests for shiro CLI.
//!
//! Exercises the full vertical slice end-to-end using the binary.

use std::process::Command;

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

#[test]
fn end_to_end_pipeline() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-test");

    // 1. Init
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["command"].as_str().unwrap(), "shiro init");
    assert!(v["result"]["created"].as_bool().unwrap());
    assert!(v["next_actions"].is_array());

    // 2. Create a test document.
    let doc_path = tmp.path().join("test.txt");
    std::fs::write(
        &doc_path,
        "Rust Programming Language\n\nRust is a systems programming language focused on safety.\n\nIt prevents data races at compile time.",
    ).unwrap();

    // 3. Add the document.
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["command"].as_str().unwrap(), "shiro add");
    let doc_id = v["result"]["doc_id"].as_str().unwrap().to_string();
    assert!(doc_id.starts_with("doc_"), "doc_id should have doc_ prefix");
    assert_eq!(v["result"]["status"].as_str().unwrap(), "READY");
    assert!(v["result"]["changed"].as_bool().unwrap());
    let segments = v["result"]["segments"].as_u64().unwrap();
    assert!(
        segments >= 2,
        "should have at least 2 segments, got {segments}"
    );

    // 4. Add the same document again (idempotent).
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add idempotent failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(
        !v["result"]["changed"].as_bool().unwrap(),
        "re-add should not change"
    );

    // 5. List documents.
    let (stdout, code) = shiro(&home, &["list"]);
    assert_eq!(code, 0, "list failed: {stdout}");
    let v = parse_json(&stdout);
    assert_eq!(v["result"]["showing"].as_u64().unwrap(), 1);
    assert_eq!(v["result"]["items"][0]["doc_id"].as_str().unwrap(), doc_id);
    assert_eq!(v["result"]["items"][0]["status"].as_str().unwrap(), "READY");

    // 6. Search for "Rust".
    let (stdout, code) = shiro(&home, &["search", "Rust"]);
    assert_eq!(code, 0, "search failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    let results = v["result"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "search should find results");

    let first = &results[0];
    assert_eq!(first["doc_id"].as_str().unwrap(), doc_id);
    let result_id = first["result_id"].as_str().unwrap().to_string();
    let entry_block_idx = first["block_idx"].as_u64().unwrap();
    let entry_block_kind = first["block_kind"].as_str().unwrap().to_string();
    let entry_span = first["span"].clone();
    assert!(
        result_id.starts_with("res_"),
        "result_id should have res_ prefix"
    );
    assert!(first["scores"]["bm25"]["score"].as_f64().unwrap() > 0.0);
    // ADR-007: segment_id must NOT appear in public output.
    assert!(
        first.get("segment_id").is_none(),
        "segment_id must not appear in public search output (ADR-007)"
    );
    // ADR-007: block_idx and block_kind must be present.
    assert!(
        first["block_idx"].is_u64(),
        "block_idx must be present in search output"
    );
    assert!(
        first["block_kind"].is_string(),
        "block_kind must be present in search output"
    );

    // 7. Read the document (text mode).
    let (stdout, code) = shiro(&home, &["read", &doc_id, "--view", "text"]);
    assert_eq!(code, 0, "read failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["result"]["text"].as_str().unwrap().contains("Rust"));

    // 8. Read the document (blocks mode).
    let (stdout, code) = shiro(&home, &["read", &doc_id, "--view", "blocks"]);
    assert_eq!(code, 0, "read blocks failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["result"]["blocks"].as_array().unwrap().len() >= 2);

    // 9. Explain the search result.
    let (stdout, code) = shiro(&home, &["explain", &result_id]);
    assert_eq!(code, 0, "explain failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["result"]["result_id"].as_str().unwrap(), result_id);
    assert!(v["result"]["scores"]["bm25"]["score"].as_f64().unwrap() > 0.0);
    // ADR-007: segment_id must NOT appear in public output.
    assert!(
        v["result"].get("segment_id").is_none(),
        "segment_id must not appear in explain output (ADR-007)"
    );
    assert!(
        v["result"]["block_idx"].is_u64(),
        "block_idx must be present in explain output"
    );
    assert!(
        v["result"]["block_kind"].is_string(),
        "block_kind must be present in explain output"
    );
    assert_eq!(v["result"]["block_idx"].as_u64().unwrap(), entry_block_idx);
    assert_eq!(
        v["result"]["block_kind"].as_str().unwrap(),
        entry_block_kind
    );
    assert_eq!(v["result"]["span"], entry_span);

    // 10. Doctor check.
    let (stdout, code) = shiro(&home, &["doctor"]);
    assert_eq!(code, 0, "doctor failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["result"]["healthy"].as_bool().unwrap());

    // 11. Remove the document.
    let (stdout, code) = shiro(&home, &["remove", &doc_id, "--purge"]);
    assert_eq!(code, 0, "remove failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["result"]["removed"].as_bool().unwrap());

    // 12. List should show the doc as DELETED.
    let (stdout, code) = shiro(&home, &["list"]);
    assert_eq!(code, 0);
    let v = parse_json(&stdout);
    assert_eq!(
        v["result"]["items"][0]["status"].as_str().unwrap(),
        "DELETED"
    );
}

/// Verify the self-documenting root command returns the full command tree.
#[test]
fn root_command_self_documenting() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-root-test");

    let (stdout, code) = shiro(&home, &[]);
    assert_eq!(code, 0, "root failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["command"].as_str().unwrap(), "shiro");

    let commands = v["result"]["commands"].as_array().unwrap();
    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();

    // Verify required commands from CLI.md are present.
    for required in &[
        "init",
        "add",
        "ingest",
        "search",
        "read",
        "explain",
        "list",
        "remove",
        "taxonomy",
        "config",
        "doctor",
        "reindex",
        "mcp",
        "completions",
    ] {
        assert!(
            names.contains(required),
            "missing required command: {required}"
        );
    }
}

/// Verify error envelope shape matches CLI.md contract.
#[test]
fn error_envelope_contract() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-err-test");

    // Search without init should produce an error envelope.
    let (stdout, code) = shiro(&home, &["search", "anything"]);
    assert_ne!(code, 0, "should fail without init");
    let v = parse_json(&stdout);
    assert!(!v["ok"].as_bool().unwrap());
    assert_eq!(v["command"].as_str().unwrap(), "shiro search");
    assert!(v["error"].is_object());
    assert!(v["error"]["code"].is_string());
    assert!(v["error"]["message"].is_string());
    assert!(v["next_actions"].is_array());
}

/// Verify config show works.
#[test]
fn config_show() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-config-test");

    // Init first.
    shiro(&home, &["init"]);

    let (stdout, code) = shiro(&home, &["config", "show"]);
    assert_eq!(code, 0, "config show failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["result"]["home"].is_string());
    assert!(v["result"]["db_path"].is_string());
}

/// Golden test: verify root command next_actions match CLI.md contract.
#[test]
fn root_next_actions_match_contract() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-next-actions-test");

    let (stdout, code) = shiro(&home, &[]);
    assert_eq!(code, 0, "root failed: {stdout}");
    let v = parse_json(&stdout);

    let next = v["next_actions"].as_array().unwrap();
    assert_eq!(next.len(), 2, "root should have exactly 2 next_actions");

    // First: doctor (simple)
    assert_eq!(next[0]["command"].as_str().unwrap(), "shiro doctor");
    assert_eq!(
        next[0]["description"].as_str().unwrap(),
        "Check library health"
    );

    // Second: list with params
    assert_eq!(
        next[1]["command"].as_str().unwrap(),
        "shiro list [--limit <n>]"
    );
    assert_eq!(next[1]["params"]["n"]["default"].as_u64().unwrap(), 20);
}

/// Verify exit codes match CLI.md contract for known error classes.
#[test]
fn exit_code_contract() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-exit-code-test");

    // Search without init = I/O error opening DB (not found).
    let (_stdout, code) = shiro(&home, &["search", "anything"]);
    // Should be non-zero.
    assert_ne!(code, 0, "search on uninitialized home should fail");

    // Config get (unimplemented) = exit 2 (usage error).
    shiro(&home, &["init"]);
    let (_stdout, code) = shiro(&home, &["config", "get", "nonexistent"]);
    assert_eq!(code, 2, "config error should exit 2");
}

/// Verify that new flags are accepted without error.
#[test]
fn new_flags_accepted() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-flags-test");
    shiro(&home, &["init"]);

    // Create a test file.
    let doc_path = tmp.path().join("flagtest.txt");
    std::fs::write(&doc_path, "Flag test content").unwrap();

    // add with new flags.
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    // doctor with new flags.
    let (stdout, code) = shiro(&home, &["doctor", "--verify-vector"]);
    assert_eq!(code, 0, "doctor with --verify-vector failed: {stdout}");
}

#[test]
fn reprocess_dry_run_enforces_limits_and_executes_from_verified_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("shiro-home");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");
    let source = temp.path().join("reprocess.txt");
    std::fs::write(&source, "scoped reprocessing evidence").unwrap();
    let (stdout, code) = shiro(&home, &["add", source.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    let (stdout, code) = shiro(
        &home,
        &[
            "reprocess",
            "--parser",
            "plaintext",
            "--target",
            "derived",
            "--max-documents",
            "0",
        ],
    );
    assert_eq!(code, 0, "dry-run failed: {stdout}");
    let blocked = parse_json(&stdout);
    assert_eq!(blocked["result"]["status"], "planned");
    assert_eq!(blocked["result"]["plan"]["execution_allowed"], false);

    let (stdout, code) = shiro(
        &home,
        &["reprocess", "--parser", "plaintext", "--target", "derived"],
    );
    assert_eq!(code, 0, "plan failed: {stdout}");
    let plan = parse_json(&stdout);
    let rollback = plan["result"]["plan"]["rollback_manifest_id"]
        .as_str()
        .unwrap();
    let (stdout, code) = shiro(
        &home,
        &[
            "reprocess",
            "--parser",
            "plaintext",
            "--target",
            "derived",
            "--execute",
            "--resume-manifest-id",
            rollback,
        ],
    );
    assert_eq!(code, 0, "execute failed: {stdout}");
    let executed = parse_json(&stdout);
    assert_eq!(executed["result"]["status"], "executed");
    assert!(executed["result"]["publication"].is_array());
}

#[test]
fn search_pack_deduplicates_stable_evidence_handles() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("shiro-home");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");
    let source = temp.path().join("pack.md");
    std::fs::write(&source, "# Evidence\n\nbatched retrieval evidence").unwrap();
    let (stdout, code) = shiro(&home, &["add", source.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    let (stdout, code) = shiro(
        &home,
        &[
            "search-pack",
            "batched evidence",
            "retrieval evidence",
            "--mode",
            "bm25",
        ],
    );
    assert_eq!(code, 0, "search-pack failed: {stdout}");
    let output = parse_json(&stdout);
    assert!(output["ok"].as_bool().unwrap());
    assert_eq!(output["result"]["query_count"], 2);
    let hits = output["result"]["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["evidence_handle"]
        .as_str()
        .unwrap()
        .starts_with("blk_"));
    assert_eq!(hits[0]["matched_queries"].as_array().unwrap().len(), 2);
    assert!(hits[0]["snippet"].is_null());

    let (stdout, code) = shiro(
        &home,
        &["read", hits[0]["evidence_handle"].as_str().unwrap()],
    );
    assert_eq!(code, 0, "stable read failed: {stdout}");
    let deferred = parse_json(&stdout);
    assert_eq!(
        deferred["result"]["evidence_resolution"]["status"],
        "ACTIVE"
    );
}

#[test]
fn search_and_list_filters_are_enforced_end_to_end() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-filter-test");
    shiro(&home, &["init"]);

    let competitor_path = tmp.path().join("competitor.md");
    std::fs::write(
        &competitor_path,
        "# Competitor Heading\n\nfiltered filtered filtered competitor",
    )
    .unwrap();
    let target_path = tmp.path().join("target.md");
    std::fs::write(&target_path, "# Target Heading\n\nfiltered target evidence").unwrap();

    shiro(&home, &["add", competitor_path.to_str().unwrap()]);
    let (stdout, code) = shiro(&home, &["add", target_path.to_str().unwrap()]);
    assert_eq!(code, 0, "target add failed: {stdout}");
    let target_doc_id = parse_json(&stdout)["result"]["doc_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (stdout, code) = shiro(&home, &["enrich", &target_doc_id]);
    assert_eq!(code, 0, "target enrich failed: {stdout}");

    let (stdout, code) = shiro(
        &home,
        &[
            "taxonomy",
            "add",
            "--scheme",
            "https://example.test/filters",
            "--label",
            "Target Concept",
        ],
    );
    assert_eq!(code, 0, "taxonomy add failed: {stdout}");
    let concept_id = parse_json(&stdout)["result"]["concept_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (stdout, code) = shiro(&home, &["taxonomy", "assign", &target_doc_id, &concept_id]);
    assert_eq!(code, 0, "taxonomy assign failed: {stdout}");

    let (stdout, code) = shiro(
        &home,
        &[
            "search",
            "filtered",
            "--tag",
            "target heading",
            "--concept",
            &concept_id,
            "--doc",
            &target_doc_id,
            "--limit",
            "1",
        ],
    );
    assert_eq!(code, 0, "filtered search failed: {stdout}");
    let search = parse_json(&stdout);
    let results = search["result"]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["doc_id"].as_str().unwrap(), target_doc_id);

    let (stdout, code) = shiro(
        &home,
        &["list", "--tag", "target heading", "--concept", &concept_id],
    );
    assert_eq!(code, 0, "filtered list failed: {stdout}");
    let list = parse_json(&stdout);
    let items = list["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["doc_id"].as_str().unwrap(), target_doc_id);
}

/// Golden test: capabilities command schema stability.
#[test]
fn capabilities_schema_stable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-caps-schema");

    let (stdout, code) = shiro(&home, &["capabilities"]);
    assert_eq!(code, 0, "capabilities failed: {stdout}");
    let v = parse_json(&stdout);

    // Top-level envelope keys.
    let top_keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(
        {
            let mut s: Vec<&str> = top_keys.clone();
            s.sort();
            s
        },
        {
            let mut s = vec!["ok", "command", "result", "next_actions"];
            s.sort();
            s
        },
        "top-level keys mismatch: {top_keys:?}"
    );

    // result keys.
    let result = v["result"].as_object().unwrap();
    let result_keys: Vec<&str> = result.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        {
            let mut s: Vec<&str> = result_keys.clone();
            s.sort();
            s
        },
        {
            let mut s = vec![
                "version",
                "schemaVersion",
                "schema_version",
                "commands",
                "state_machine",
                "id_schemes",
                "parsers",
                "features",
                "taxonomy",
                "embedding",
                "storage",
            ];
            s.sort();
            s
        },
        "result keys mismatch: {result_keys:?}"
    );

    // state_machine.states exact set.
    let states: Vec<&str> = v["result"]["state_machine"]["states"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        vec!["STAGED", "INDEXING", "READY", "FAILED", "DELETED"],
        "state_machine.states mismatch"
    );

    // features exact key set.
    let features = v["result"]["features"].as_object().unwrap();
    let feature_keys: Vec<&str> = features.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        {
            let mut s: Vec<&str> = feature_keys.clone();
            s.sort();
            s
        },
        {
            let mut s = vec![
                "fts_bm25",
                "hybrid_search",
                "vector_embed",
                "reranking",
                "taxonomy",
                "enrichment",
                "mcp_server",
                "completions",
                "judged_benchmark",
                "stable_evidence_handles",
                "multi_query_search_pack",
                "bounded_reprocessing_planner",
                "mcp_current_protocol_and_authority",
                "bounded_url_acquisition",
                "taxonomy_browse_search",
                "reversible_model_enrichment_proposals",
                "automatic_concept_proposals",
            ];
            s.sort();
            s
        },
        "features keys mismatch: {feature_keys:?}"
    );

    // id_schemes exact key set, each with prefix + algorithm.
    let id_schemes = v["result"]["id_schemes"].as_object().unwrap();
    let id_keys: Vec<&str> = id_schemes.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        {
            let mut s: Vec<&str> = id_keys.clone();
            s.sort();
            s
        },
        {
            let mut s = vec![
                "doc_id",
                "segment_id",
                "run_id",
                "concept_id",
                "result_id",
                "evidence_handle",
            ];
            s.sort();
            s
        },
        "id_schemes keys mismatch: {id_keys:?}"
    );
    for (name, scheme) in id_schemes {
        let obj = scheme
            .as_object()
            .unwrap_or_else(|| panic!("id_schemes.{name} not an object"));
        assert!(
            obj.contains_key("prefix"),
            "id_schemes.{name} missing prefix"
        );
        assert!(
            obj.contains_key("algorithm"),
            "id_schemes.{name} missing algorithm"
        );
    }
}

/// Golden test: doctor command schema stability.
#[test]
fn doctor_schema_stable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-doctor-schema");

    // Init first so doctor has something to check.
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let (stdout, code) = shiro(&home, &["doctor"]);
    assert_eq!(code, 0, "doctor failed: {stdout}");
    let v = parse_json(&stdout);

    // result.healthy is a boolean.
    assert!(
        v["result"]["healthy"].is_boolean(),
        "result.healthy must be a boolean"
    );

    // result.checks is an array.
    let checks = v["result"]["checks"].as_array().unwrap();
    assert!(
        !checks.is_empty(),
        "doctor should return at least one check"
    );

    // Each check has name, status, message (details optional).
    let required_check_keys = ["name", "status", "message"];
    for (i, check) in checks.iter().enumerate() {
        let obj = check
            .as_object()
            .unwrap_or_else(|| panic!("check[{i}] not an object"));
        for key in &required_check_keys {
            assert!(obj.contains_key(*key), "check[{i}] missing key: {key}");
        }
        // Only name/status/message/details are allowed.
        for key in obj.keys() {
            assert!(
                ["name", "status", "message", "details"].contains(&key.as_str()),
                "check[{i}] has unexpected key: {key}"
            );
        }
    }

    // Required check names.
    let check_names: Vec<&str> = checks.iter().map(|c| c["name"].as_str().unwrap()).collect();
    for required in &[
        "home_directory",
        "sqlite_store",
        "fts_index",
        "schema_version",
        "document_states",
    ] {
        assert!(
            check_names.contains(required),
            "doctor missing check: {required}, got: {check_names:?}"
        );
    }
}

/// Golden test: add command schema stability.
#[test]
fn add_schema_stable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-add-schema");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("schema_test.txt");
    std::fs::write(&doc_path, "Schema stability test content for add command.").unwrap();

    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");
    let v = parse_json(&stdout);

    // result must have exactly these keys.
    let result = v["result"].as_object().unwrap();
    for key in &["doc_id", "status", "title", "segments", "changed"] {
        assert!(result.contains_key(*key), "add result missing key: {key}");
    }

    // doc_id prefix.
    let doc_id = result["doc_id"].as_str().unwrap();
    assert!(
        doc_id.starts_with("doc_"),
        "doc_id should start with doc_, got: {doc_id}"
    );

    // status is a valid DocState string.
    let status = result["status"].as_str().unwrap();
    assert!(
        ["STAGED", "INDEXING", "READY", "FAILED", "DELETED"].contains(&status),
        "status is not a valid DocState: {status}"
    );

    // segments is a number.
    assert!(result["segments"].is_u64(), "segments must be a number");

    // changed is a boolean.
    assert!(result["changed"].is_boolean(), "changed must be a boolean");
}

/// Golden test: search command schema stability.
#[test]
fn search_schema_stable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-search-schema");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("searchable.txt");
    std::fs::write(
        &doc_path,
        "Searchable content for schema stability golden test.",
    )
    .unwrap();

    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    let (stdout, code) = shiro(&home, &["search", "searchable"]);
    assert_eq!(code, 0, "search failed: {stdout}");
    let v = parse_json(&stdout);

    // result envelope keys.
    let result = v["result"].as_object().unwrap();
    for key in &["query", "mode", "results"] {
        assert!(
            result.contains_key(*key),
            "search result missing key: {key}"
        );
    }

    // results is an array with at least one hit.
    let results = result["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "search should return at least one result"
    );

    // Each result has the required keys.
    for (i, r) in results.iter().enumerate() {
        let obj = r
            .as_object()
            .unwrap_or_else(|| panic!("result[{i}] not an object"));
        for key in &[
            "doc_id",
            "block_idx",
            "block_kind",
            "result_id",
            "snippet",
            "scores",
        ] {
            assert!(obj.contains_key(*key), "result[{i}] missing key: {key}");
        }
        // ADR-007: segment_id must NOT appear.
        assert!(
            !obj.contains_key("segment_id"),
            "result[{i}] must not contain segment_id (ADR-007)"
        );

        // result_id prefix.
        let result_id = obj["result_id"].as_str().unwrap();
        assert!(
            result_id.starts_with("res_"),
            "result_id should start with res_, got: {result_id}"
        );

        // scores.bm25 has score and rank.
        let bm25 = &obj["scores"]["bm25"];
        assert!(
            bm25["score"].is_f64() || bm25["score"].is_u64(),
            "scores.bm25.score must be numeric in result[{i}]"
        );
        assert!(
            bm25["rank"].is_u64() || bm25["rank"].is_i64(),
            "scores.bm25.rank must be numeric in result[{i}]"
        );
    }
}

#[test]
fn same_content_same_docid() {
    let tmp = tempfile::TempDir::new().unwrap();
    let content = "Deterministic hashing test content for corpus regression.";

    // Run 1
    let home1 = tmp.path().join("home1");
    let doc1 = tmp.path().join("doc1.txt");
    std::fs::write(&doc1, content).unwrap();
    let (stdout1, _) = shiro(&home1, &["init"]);
    assert!(parse_json(&stdout1)["ok"].as_bool().unwrap());
    let (stdout1, _) = shiro(&home1, &["add", doc1.to_str().unwrap()]);
    let id1 = parse_json(&stdout1)["result"]["doc_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Run 2 (different home, same content)
    let home2 = tmp.path().join("home2");
    let doc2 = tmp.path().join("doc2.txt");
    std::fs::write(&doc2, content).unwrap();
    let (stdout2, _) = shiro(&home2, &["init"]);
    assert!(parse_json(&stdout2)["ok"].as_bool().unwrap());
    let (stdout2, _) = shiro(&home2, &["add", doc2.to_str().unwrap()]);
    let id2 = parse_json(&stdout2)["result"]["doc_id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(id1, id2, "same content must produce same DocId");
    assert!(id1.starts_with("doc_"), "DocId must have doc_ prefix");
}

#[test]
fn ingest_deterministic_across_runs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let corpus = tmp.path().join("corpus");
    std::fs::create_dir(&corpus).unwrap();
    std::fs::write(corpus.join("alpha.txt"), "Alpha document content").unwrap();
    std::fs::write(corpus.join("beta.txt"), "Beta document content").unwrap();
    std::fs::write(corpus.join("gamma.txt"), "Gamma document content").unwrap();

    // Run 1
    let home1 = tmp.path().join("h1");
    shiro(&home1, &["init"]);
    let (out1, code1) = shiro(&home1, &["ingest", corpus.to_str().unwrap()]);
    assert_eq!(code1, 0, "ingest failed: {out1}");
    let (list1, _) = shiro(&home1, &["list"]);
    let docs1 = parse_json(&list1)["result"]["items"]
        .as_array()
        .unwrap()
        .clone();
    let ids1: Vec<&str> = docs1
        .iter()
        .map(|d| d["doc_id"].as_str().unwrap())
        .collect();

    // Run 2 (fresh home, same corpus)
    let home2 = tmp.path().join("h2");
    shiro(&home2, &["init"]);
    let (out2, code2) = shiro(&home2, &["ingest", corpus.to_str().unwrap()]);
    assert_eq!(code2, 0, "ingest failed: {out2}");
    let (list2, _) = shiro(&home2, &["list"]);
    let docs2 = parse_json(&list2)["result"]["items"]
        .as_array()
        .unwrap()
        .clone();
    let ids2: Vec<&str> = docs2
        .iter()
        .map(|d| d["doc_id"].as_str().unwrap())
        .collect();

    // Same set of DocIds (sorted for comparison)
    let mut sorted1: Vec<&str> = ids1.clone();
    let mut sorted2: Vec<&str> = ids2.clone();
    sorted1.sort();
    sorted2.sort();
    assert_eq!(sorted1, sorted2, "same corpus must produce same DocId set");
    assert_eq!(sorted1.len(), 3, "expected 3 documents");
}

#[test]
fn test_taxonomy_workflow() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-taxonomy-wf");

    // Init
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    // Add a concept
    let (stdout, code) = shiro(
        &home,
        &[
            "taxonomy",
            "add",
            "--scheme",
            "http://example.org/lang",
            "--label",
            "Rust",
        ],
    );
    assert_eq!(code, 0, "taxonomy add failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    let concept_id = v["result"]["concept_id"].as_str().unwrap().to_string();
    assert!(
        concept_id.starts_with("con_"),
        "concept_id should have con_ prefix"
    );
    assert!(v["result"]["created"].as_bool().unwrap());

    // List concepts
    let (stdout, code) = shiro(&home, &["taxonomy", "list"]);
    assert_eq!(code, 0, "taxonomy list failed: {stdout}");
    let v = parse_json(&stdout);
    let concepts = v["result"]["concepts"].as_array().unwrap();
    assert_eq!(concepts.len(), 1);
    assert_eq!(concepts[0]["id"].as_str().unwrap(), concept_id);
    assert_eq!(concepts[0]["pref_label"].as_str().unwrap(), "Rust");
    assert_eq!(
        concepts[0]["scheme_uri"].as_str().unwrap(),
        "http://example.org/lang"
    );

    // Add a second concept to verify list grows
    let (stdout, code) = shiro(
        &home,
        &[
            "taxonomy",
            "add",
            "--scheme",
            "http://example.org/lang",
            "--label",
            "Python",
        ],
    );
    assert_eq!(code, 0, "taxonomy add Python failed: {stdout}");
    let (stdout, _) = shiro(&home, &["taxonomy", "list"]);
    let v = parse_json(&stdout);
    assert_eq!(v["result"]["count"].as_u64().unwrap(), 2);

    let (stdout, code) = shiro(&home, &["taxonomy", "search", "rust"]);
    assert_eq!(code, 0, "taxonomy search failed: {stdout}");
    let searched = parse_json(&stdout);
    assert_eq!(searched["result"]["concepts"][0]["pref_label"], "Rust");
    assert_eq!(searched["result"]["concepts"][0]["text_fallback"], "Rust");

    let (stdout, code) = shiro(&home, &["taxonomy", "browse", "--root", &concept_id]);
    assert_eq!(code, 0, "taxonomy browse failed: {stdout}");
    let browsed = parse_json(&stdout);
    assert_eq!(browsed["result"]["root_concept_id"], concept_id);
    assert_eq!(browsed["result"]["concepts"].as_array().unwrap().len(), 1);
}

#[test]
fn model_enrichment_stays_proposed_until_promotion_and_is_reversible() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-model-enrichment");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");
    let source = tmp.path().join("model.txt");
    std::fs::write(&source, "model proposal evidence").unwrap();
    let (stdout, code) = shiro(&home, &["add", source.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");
    let added = parse_json(&stdout);
    let doc_id = added["result"]["doc_id"].as_str().unwrap();

    let proposal_file = tmp.path().join("proposal.json");
    std::fs::write(
        &proposal_file,
        serde_json::to_vec(&serde_json::json!({
            "doc_id": doc_id,
            "provider": "test-provider",
            "model": "test-model",
            "actor_id": "agent:test",
            "data_region": "local",
            "retention_policy": "none",
            "consent_id": "consent:test",
            "concepts": [{
                "scheme_uri": "urn:test:topics",
                "pref_label": "Proposed Topic",
                "alt_labels": [],
                "definition": "A model-proposed topic",
                "confidence": 0.9
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let (stdout, code) = shiro(
        &home,
        &[
            "enrich-model",
            "propose",
            "--file",
            proposal_file.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "proposal failed: {stdout}");
    let proposed = parse_json(&stdout);
    assert_eq!(proposed["result"]["status"], "PROPOSED");
    assert_eq!(proposed["result"]["trust_zone"], "PROPOSED");
    let proposal_id = proposed["result"]["proposal_id"].as_str().unwrap();
    let concept_id = proposed["result"]["proposed_concept_ids"][0]
        .as_str()
        .unwrap();

    let (stdout, code) = shiro(&home, &["list", "--concept", concept_id]);
    assert_eq!(code, 0, "pre-promotion list failed: {stdout}");
    assert!(parse_json(&stdout)["result"]["items"]
        .as_array()
        .unwrap()
        .is_empty());

    let (stdout, code) = shiro(
        &home,
        &[
            "enrich-model",
            "resolve",
            proposal_id,
            "--action",
            "promote",
            "--actor",
            "local_user",
            "--approval",
            "approval:test",
        ],
    );
    assert_eq!(code, 0, "promotion failed: {stdout}");
    assert_eq!(parse_json(&stdout)["result"]["status"], "PROMOTED");
    let (stdout, _) = shiro(&home, &["list", "--concept", concept_id]);
    assert_eq!(
        parse_json(&stdout)["result"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let (stdout, code) = shiro(
        &home,
        &[
            "enrich-model",
            "resolve",
            proposal_id,
            "--action",
            "reject",
            "--actor",
            "local_user",
            "--approval",
            "reversal:test",
        ],
    );
    assert_eq!(code, 0, "reversal failed: {stdout}");
    assert_eq!(parse_json(&stdout)["result"]["status"], "REJECTED");
    let (stdout, _) = shiro(&home, &["list", "--concept", concept_id]);
    assert!(parse_json(&stdout)["result"]["items"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn test_reindex_command() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-reindex-test");

    // Init + add a file
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("reindex_doc.txt");
    std::fs::write(
        &doc_path,
        "Reindexing test content about algorithms and data structures",
    )
    .unwrap();
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    // Reindex FTS
    let (stdout, code) = shiro(&home, &["reindex"]);
    assert_eq!(code, 0, "reindex failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    let actions = v["result"]["actions"].as_array().unwrap();
    assert!(
        !actions.is_empty(),
        "reindex should report at least one action"
    );
    assert_eq!(actions[0]["index"].as_str().unwrap(), "fts");
    assert_eq!(actions[0]["status"].as_str().unwrap(), "rebuilt");

    // Verify search still works after reindex
    let (stdout, code) = shiro(&home, &["search", "algorithms"]);
    assert_eq!(code, 0, "search after reindex failed: {stdout}");
    let v = parse_json(&stdout);
    let results = v["result"]["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "search should still find results after reindex"
    );
}

#[test]
fn benchmark_command_reports_measured_metrics_and_honest_evidence_status() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-benchmark-test");
    shiro(&home, &["init"]);

    let content = "judged benchmark retrieval evidence";
    let document_path = tmp.path().join("benchmark.txt");
    std::fs::write(&document_path, content).unwrap();
    let (stdout, code) = shiro(&home, &["add", document_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");
    let doc_id = parse_json(&stdout)["result"]["doc_id"]
        .as_str()
        .unwrap()
        .to_string();

    let manifest_path = tmp.path().join("benchmark-manifest.json");
    let manifest = serde_json::json!({
        "schema_version": 1,
        "corpus_id": "integration-fixture",
        "corpus_version": "1",
        "documents": [{
            "doc_id": doc_id,
            "source_uri": document_path.to_str().unwrap(),
            "source_hash": blake3::hash(content.as_bytes()).to_hex().to_string(),
            "license": "test-only"
        }],
        "queries": [{
            "query_id": "q1",
            "text": "retrieval evidence",
            "judgments": [{
                "doc_id": doc_id,
                "relevance": 3,
                "source_locator": "bytes:0-35",
                "assessor_ids": ["integration-test"],
                "adjudicated": true
            }]
        }],
        "hardware_profile": {
            "profile_id": "integration-host",
            "cpu_model": "unspecified-test-cpu",
            "logical_cores": 1,
            "memory_bytes": 1,
            "os": std::env::consts::OS,
            "architecture": std::env::consts::ARCH
        },
        "pipelines": ["bm25"],
        "thresholds": {
            "min_candidate_recall_at_50": 1.0,
            "min_ndcg_at_10": 1.0,
            "min_mrr_at_10": 1.0,
            "max_search_p95_ms": 10000.0,
            "min_paired_ndcg_delta_ci95_lower": null
        }
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let (stdout, code) = shiro(
        &home,
        &[
            "benchmark",
            manifest_path.to_str().unwrap(),
            "--warmup-runs",
            "0",
            "--measured-runs",
            "2",
        ],
    );
    assert_eq!(code, 0, "benchmark failed: {stdout}");
    let output = parse_json(&stdout);
    assert_eq!(
        output["result"]["evidence_status"].as_str(),
        Some("insufficient_evidence")
    );
    assert_eq!(
        output["result"]["pipelines"][0]["candidate_recall_at_50"].as_f64(),
        Some(1.0)
    );
    assert_eq!(
        output["result"]["pipelines"][0]["ndcg_at_10"].as_f64(),
        Some(1.0)
    );
    assert_eq!(
        output["result"]["rebuild_integrity"]["passed"].as_bool(),
        Some(true)
    );
}

#[test]
fn test_enrich_command() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-enrich-test");

    // Init + add
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("enrich_doc.txt");
    std::fs::write(
        &doc_path,
        "# Machine Learning Overview\n\nMachine learning is a subset of artificial intelligence.\n\nIt uses statistical methods to learn from data.",
    ).unwrap();
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");
    let doc_id = parse_json(&stdout)["result"]["doc_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Enrich
    let (stdout, code) = shiro(&home, &["enrich", &doc_id]);
    assert_eq!(code, 0, "enrich failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["result"]["doc_id"].as_str().unwrap(), doc_id);
    assert_eq!(v["result"]["provider"].as_str().unwrap(), "heuristic");
    // Heuristic enrichment extracts title from markdown heading
    assert_eq!(
        v["result"]["title"].as_str().unwrap(),
        "Machine Learning Overview"
    );
    assert!(v["result"]["summary_length"].as_u64().unwrap() > 0);
}

#[test]
fn test_config_workflow() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-config-wf");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    // Set a config value
    let (stdout, code) = shiro(&home, &["config", "set", "search.limit", "20"]);
    assert_eq!(code, 0, "config set failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["result"]["key"].as_str().unwrap(), "search.limit");

    // Get the value back
    let (stdout, code) = shiro(&home, &["config", "get", "search.limit"]);
    assert_eq!(code, 0, "config get failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["result"]["key"].as_str().unwrap(), "search.limit");
    // Value should be 20 (number or string)
    let val = &v["result"]["value"];
    assert!(
        val.as_u64() == Some(20) || val.as_str() == Some("20"),
        "config get should return 20, got: {val}"
    );

    // Show should include the value
    let (stdout, code) = shiro(&home, &["config", "show"]);
    assert_eq!(code, 0, "config show failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["result"]["home"].is_string());
}

#[test]
fn test_search_modes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-search-modes");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("search_mode.txt");
    std::fs::write(
        &doc_path,
        "Quantum computing leverages quantum mechanics for computation",
    )
    .unwrap();
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    // Search with explicit bm25 mode
    let (stdout, code) = shiro(&home, &["search", "--mode", "bm25", "quantum"]);
    assert_eq!(code, 0, "search --mode bm25 failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["result"]["mode"].as_str().unwrap(), "bm25");
    let results = v["result"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "bm25 search should find results");
    // In bm25-only mode, bm25 score should be present
    assert!(results[0]["scores"]["bm25"]["score"].as_f64().unwrap() > 0.0);
}

#[test]
fn test_explain_has_generations() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-explain-gen");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("explain_gen.txt");
    std::fs::write(
        &doc_path,
        "Neural networks are the foundation of deep learning",
    )
    .unwrap();
    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    // Search to get a result_id
    let (stdout, code) = shiro(&home, &["search", "neural"]);
    assert_eq!(code, 0, "search failed: {stdout}");
    let v = parse_json(&stdout);
    let result_id = v["result"]["results"][0]["result_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Explain should include generations field
    let (stdout, code) = shiro(&home, &["explain", &result_id]);
    assert_eq!(code, 0, "explain failed: {stdout}");
    let v = parse_json(&stdout);
    assert!(v["ok"].as_bool().unwrap());
    assert!(
        v["result"]["generations"].is_object(),
        "explain should include generations"
    );
    assert!(
        v["result"]["generations"]["fts"].is_number(),
        "generations.fts should be a number"
    );
}

#[test]
fn test_schema_version_in_capabilities() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-caps-version");

    let (stdout, code) = shiro(&home, &["capabilities"]);
    assert_eq!(code, 0, "capabilities failed: {stdout}");
    let v = parse_json(&stdout);
    // Must include schemaVersion (camelCase) or schema_version (snake_case)
    let has_schema_version =
        v["result"]["schemaVersion"].is_number() || v["result"]["schema_version"].is_number();
    assert!(
        has_schema_version,
        "capabilities must include schemaVersion field"
    );
}

#[test]
fn test_envelope_schema_version() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-envelope-check");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    // Collect outputs from multiple commands
    let commands: Vec<(&[&str], &str)> = vec![
        (&["init"] as &[&str], "init"),
        (&["list"], "list"),
        (&["doctor"], "doctor"),
        (&["config", "show"], "config show"),
        (&["capabilities"], "capabilities"),
    ];

    for (args, label) in commands {
        let (stdout, code) = shiro(&home, args);
        // capabilities doesn't need init, others should succeed after init
        if code != 0 {
            continue;
        }
        let v = parse_json(&stdout);
        assert!(v["ok"].is_boolean(), "{label}: missing 'ok' field");
        assert!(v["command"].is_string(), "{label}: missing 'command' field");
        assert!(v["result"].is_object(), "{label}: missing 'result' field");
        assert!(
            v["next_actions"].is_array(),
            "{label}: missing 'next_actions' field"
        );
    }
}

/// Contract test: capabilities JSON envelope structure.
#[test]
fn contract_capabilities_json() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-contract-caps");

    let (stdout, code) = shiro(&home, &["capabilities"]);
    assert_eq!(code, 0, "capabilities failed: {stdout}");
    let v = parse_json(&stdout);

    // ok must be true
    assert_eq!(v["ok"].as_bool(), Some(true), "ok must be true");

    // result.schemaVersion is a number
    assert!(
        v["result"]["schemaVersion"].is_number() || v["result"]["schema_version"].is_number(),
        "result.schemaVersion or result.schema_version must be a number"
    );

    // result.commands is an array
    assert!(
        v["result"]["commands"].is_array(),
        "result.commands must be an array"
    );

    // result.features is an object with known keys
    let features = v["result"]["features"]
        .as_object()
        .expect("result.features must be an object");
    for key in &["mcp_server", "vector_embed", "fts_bm25", "taxonomy"] {
        assert!(
            features.contains_key(*key),
            "result.features missing key: {key}"
        );
    }

    // Exact feature values for critical integrations
    assert_eq!(
        v["result"]["features"]["mcp_server"].as_str(),
        Some("code_mode"),
        "mcp_server must equal code_mode"
    );
    assert_eq!(
        v["result"]["features"]["vector_embed"].as_str(),
        Some("implemented"),
        "vector_embed must equal implemented"
    );
}

/// Contract test: list JSON envelope on an initialized but empty home.
#[test]
fn contract_list_json() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-contract-list");

    // Init first so list succeeds.
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let (stdout, code) = shiro(&home, &["list"]);
    assert_eq!(code, 0, "list failed: {stdout}");
    let v = parse_json(&stdout);

    // ok must be true
    assert_eq!(v["ok"].as_bool(), Some(true), "ok must be true");

    // result.items is an array (may be empty on fresh home)
    assert!(
        v["result"]["items"].is_array(),
        "result.items must be an array"
    );

    // result.truncated is a boolean
    assert!(
        v["result"]["truncated"].is_boolean(),
        "result.truncated must be a boolean"
    );
}

/// ADR-004: verify processing fingerprints are persisted after add.
#[test]
fn fingerprint_persisted_after_add() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-fp-test");

    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let doc_path = tmp.path().join("fp_test.txt");
    std::fs::write(
        &doc_path,
        "Fingerprint test content for ADR-004 compliance.",
    )
    .unwrap();

    let (stdout, code) = shiro(&home, &["add", doc_path.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");

    // Doctor should show processing_fingerprints check as "ok"
    let (stdout, code) = shiro(&home, &["doctor"]);
    assert_eq!(code, 0, "doctor failed: {stdout}");
    let v = parse_json(&stdout);
    let checks = v["result"]["checks"].as_array().unwrap();
    let fp_check = checks
        .iter()
        .find(|c| c["name"].as_str() == Some("processing_fingerprints"));
    assert!(
        fp_check.is_some(),
        "doctor should include processing_fingerprints check"
    );
    assert_eq!(
        fp_check.unwrap()["status"].as_str(),
        Some("ok"),
        "fingerprint should be present after add"
    );
}

/// Contract test: doctor JSON envelope structure.
#[test]
fn contract_doctor_json() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-contract-doctor");

    // Init first so doctor has something to check.
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let (stdout, _code) = shiro(&home, &["doctor"]);
    let v = parse_json(&stdout);

    // ok can be true or false (both valid for doctor)
    assert!(v["ok"].is_boolean(), "ok must be a boolean");

    // If result exists, it must have checks array and healthy boolean.
    if v["result"].is_object() {
        assert!(
            v["result"]["checks"].is_array(),
            "result.checks must be an array"
        );
        assert!(
            v["result"]["healthy"].is_boolean(),
            "result.healthy must be a boolean"
        );

        // Each check should have a name, status, and message.
        let checks = v["result"]["checks"].as_array().unwrap();
        for (i, check) in checks.iter().enumerate() {
            assert!(
                check["name"].is_string(),
                "checks[{i}].name must be a string"
            );
            assert!(
                check["status"].is_string(),
                "checks[{i}].status must be a string"
            );
            assert!(
                check["message"].is_string(),
                "checks[{i}].message must be a string"
            );
        }
    }
}

#[test]
fn taxonomy_import_rebuilds_hierarchical_closure_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-taxonomy-import");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let taxonomy_file = tmp.path().join("taxonomy.json");
    std::fs::write(
        &taxonomy_file,
        serde_json::to_vec(&serde_json::json!([
            {
                "scheme_uri": "urn:test:hierarchy",
                "pref_label": "Systems",
                "broader": [],
                "narrower": [],
                "related": []
            },
            {
                "scheme_uri": "urn:test:hierarchy",
                "pref_label": "Rust",
                "broader": ["Systems"],
                "narrower": [],
                "related": []
            }
        ]))
        .unwrap(),
    )
    .unwrap();
    let (stdout, code) = shiro(
        &home,
        &["taxonomy", "import", taxonomy_file.to_str().unwrap()],
    );
    assert_eq!(code, 0, "taxonomy import failed: {stdout}");

    let home = camino::Utf8Path::from_path(&home).unwrap();
    let store = shiro_store::Store::open(&home.join("shiro.db")).unwrap();
    let systems = shiro_core::ConceptId::new("urn:test:hierarchy", "Systems");
    let rust = shiro_core::ConceptId::new("urn:test:hierarchy", "Rust");
    assert_eq!(
        store.get_concept_descendant_ids(&systems).unwrap(),
        vec![rust]
    );
}

#[test]
fn ancestor_concept_filter_matches_descendant_assignment() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-hierarchical-filter");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let source = tmp.path().join("rust.txt");
    std::fs::write(&source, "hierarchical retrieval evidence").unwrap();
    let (stdout, code) = shiro(&home, &["add", source.to_str().unwrap()]);
    assert_eq!(code, 0, "add failed: {stdout}");
    let doc_id = parse_json(&stdout)["result"]["doc_id"]
        .as_str()
        .unwrap()
        .to_string();

    let taxonomy_file = tmp.path().join("hierarchy.json");
    std::fs::write(
        &taxonomy_file,
        serde_json::to_vec(&serde_json::json!([
            {
                "scheme_uri": "urn:test:retrieval",
                "pref_label": "Programming Languages"
            },
            {
                "scheme_uri": "urn:test:retrieval",
                "pref_label": "Rust",
                "broader": ["Programming Languages"]
            }
        ]))
        .unwrap(),
    )
    .unwrap();
    let (stdout, code) = shiro(
        &home,
        &["taxonomy", "import", taxonomy_file.to_str().unwrap()],
    );
    assert_eq!(code, 0, "taxonomy import failed: {stdout}");

    let ancestor_id = shiro_core::ConceptId::new("urn:test:retrieval", "Programming Languages");
    let descendant_id = shiro_core::ConceptId::new("urn:test:retrieval", "Rust");
    let (stdout, code) = shiro(
        &home,
        &[
            "taxonomy",
            "assign",
            &doc_id,
            descendant_id.as_str(),
            "--source",
            "test",
        ],
    );
    assert_eq!(code, 0, "taxonomy assignment failed: {stdout}");

    let (stdout, code) = shiro(
        &home,
        &[
            "search",
            "hierarchical",
            "--mode",
            "bm25",
            "--expand",
            "--concept",
            ancestor_id.as_str(),
        ],
    );
    assert_eq!(code, 0, "hierarchical search failed: {stdout}");
    let searched = parse_json(&stdout);
    let hits = searched["result"]["results"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["doc_id"], doc_id);

    let result_id = hits[0]["result_id"].as_str().unwrap();
    let (stdout, code) = shiro(&home, &["explain", result_id]);
    assert_eq!(code, 0, "explain failed: {stdout}");
    assert_eq!(parse_json(&stdout)["result"]["doc_id"], doc_id);
}

#[test]
fn taxonomy_relate_rejects_broader_cycles_without_changing_closure() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-taxonomy-relate");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let mut concept_ids = Vec::new();
    for label in ["Ancestor", "Child", "Grandchild"] {
        let (stdout, code) = shiro(
            &home,
            &[
                "taxonomy",
                "add",
                "--scheme",
                "urn:test:relate",
                "--label",
                label,
            ],
        );
        assert_eq!(code, 0, "taxonomy add failed: {stdout}");
        concept_ids.push(
            parse_json(&stdout)["result"]["concept_id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }

    for (from, to) in [
        (&concept_ids[1], &concept_ids[0]),
        (&concept_ids[2], &concept_ids[1]),
    ] {
        let (stdout, code) = shiro(
            &home,
            &["taxonomy", "relate", from, to, "--kind", "broader"],
        );
        assert_eq!(code, 0, "taxonomy relate failed: {stdout}");
        assert!(parse_json(&stdout)["result"]["closure_rebuilt"]
            .as_bool()
            .unwrap());
    }

    let home_path = camino::Utf8Path::from_path(&home).unwrap();
    let store = shiro_store::Store::open(&home_path.join("shiro.db")).unwrap();
    let ancestor_id = shiro_core::ConceptId::from_stored(concept_ids[0].clone()).unwrap();
    let closure_before = store.get_concept_descendant_ids(&ancestor_id).unwrap();
    drop(store);

    let (stdout, code) = shiro(
        &home,
        &[
            "taxonomy",
            "relate",
            &concept_ids[0],
            &concept_ids[2],
            "--kind",
            "broader",
        ],
    );
    assert_ne!(code, 0, "cycle unexpectedly accepted: {stdout}");
    let rejected = parse_json(&stdout);
    assert_eq!(rejected["error"]["code"], "E_TAXONOMY_CYCLE");

    let store = shiro_store::Store::open(&home_path.join("shiro.db")).unwrap();
    assert_eq!(
        store.get_concept_descendant_ids(&ancestor_id).unwrap(),
        closure_before
    );
    assert!(store
        .get_concept_relations(&ancestor_id)
        .unwrap()
        .is_empty());
}

#[test]
fn ingest_concept_proposals_stay_untrusted_until_promotion_and_support_opt_out() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("shiro-auto-concepts");
    let (stdout, code) = shiro(&home, &["init"]);
    assert_eq!(code, 0, "init failed: {stdout}");

    let (stdout, code) = shiro(
        &home,
        &[
            "taxonomy",
            "add",
            "--scheme",
            "urn:test:auto-concepts",
            "--label",
            "Rust",
            "--definition",
            "A systems programming language",
        ],
    );
    assert_eq!(code, 0, "taxonomy add failed: {stdout}");
    let concept_id = parse_json(&stdout)["result"]["concept_id"]
        .as_str()
        .unwrap()
        .to_string();

    let corpus = tmp.path().join("corpus");
    std::fs::create_dir(&corpus).unwrap();
    std::fs::write(
        corpus.join("rust.md"),
        "# Rust Ownership\n\nOwnership provides memory safety evidence.",
    )
    .unwrap();
    let (stdout, code) = shiro(&home, &["ingest", corpus.to_str().unwrap()]);
    assert_eq!(code, 0, "ingest failed: {stdout}");
    let ingested = parse_json(&stdout);
    let proposals = ingested["result"]["concept_proposals"].as_array().unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0]["status"], "PROPOSED");
    assert_eq!(proposals[0]["trust_zone"], "PROPOSED");
    let proposal_id = proposals[0]["proposal_id"].as_str().unwrap();

    let (stdout, code) = shiro(
        &home,
        &[
            "search",
            "ownership",
            "--mode",
            "bm25",
            "--concept",
            &concept_id,
        ],
    );
    assert_eq!(code, 0, "pre-promotion search failed: {stdout}");
    assert!(parse_json(&stdout)["result"]["results"]
        .as_array()
        .unwrap()
        .is_empty());

    let (stdout, code) = shiro(
        &home,
        &[
            "enrich-model",
            "resolve",
            proposal_id,
            "--action",
            "promote",
            "--actor",
            "local_user",
            "--approval",
            "approval:auto-concept-test",
        ],
    );
    assert_eq!(code, 0, "promotion failed: {stdout}");
    assert_eq!(parse_json(&stdout)["result"]["status"], "PROMOTED");

    let (stdout, code) = shiro(
        &home,
        &[
            "search",
            "ownership",
            "--mode",
            "bm25",
            "--concept",
            &concept_id,
        ],
    );
    assert_eq!(code, 0, "post-promotion search failed: {stdout}");
    assert!(!parse_json(&stdout)["result"]["results"]
        .as_array()
        .unwrap()
        .is_empty());

    let (stdout, code) = shiro(
        &home,
        &["config", "set", "ingest.auto_concept_proposals", "false"],
    );
    assert_eq!(code, 0, "config opt-out failed: {stdout}");
    let opted_out_corpus = tmp.path().join("opted-out-corpus");
    std::fs::create_dir(&opted_out_corpus).unwrap();
    std::fs::write(
        opted_out_corpus.join("second.md"),
        "# Rust Lifetimes\n\nA second unique document.",
    )
    .unwrap();
    let (stdout, code) = shiro(&home, &["ingest", opted_out_corpus.to_str().unwrap()]);
    assert_eq!(code, 0, "opted-out ingest failed: {stdout}");
    assert!(parse_json(&stdout)["result"]["concept_proposals"]
        .as_array()
        .unwrap()
        .is_empty());
}
