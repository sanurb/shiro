//! Integration test: init → add → search → read → explain → list → remove → doctor.
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
    assert!(
        result_id.starts_with("res_"),
        "result_id should have res_ prefix"
    );
    assert!(first["scores"]["bm25"]["score"].as_f64().unwrap() > 0.0);

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
    assert!(v["result"]["expansion"].is_object());

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
    let (stdout, code) = shiro(
        &home,
        &[
            "add",
            doc_path.to_str().unwrap(),
            "--parser",
            "baseline",
            "--fts-only",
        ],
    );
    assert_eq!(code, 0, "add with new flags failed: {stdout}");

    // doctor with new flags.
    let (stdout, code) = shiro(&home, &["doctor", "--verify-vector"]);
    assert_eq!(code, 0, "doctor with --verify-vector failed: {stdout}");
}
