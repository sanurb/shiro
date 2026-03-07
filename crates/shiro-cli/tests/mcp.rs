//! MCP Code Mode integration tests.
//!
//! These tests exercise the full JSON-RPC 2.0 MCP server over stdio,
//! validating round-trip contracts, error mapping, and determinism.

use std::io::Write;
use std::process::{Command, Stdio};

/// Send JSON-RPC requests to `shiro mcp` and collect responses.
fn mcp_roundtrip(home: &std::path::Path, requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let bin = env!("CARGO_BIN_EXE_shiro");

    let mut child = Command::new(bin)
        .args(["--home", home.to_str().unwrap()])
        .args(["--log-level", "silent"])
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn shiro mcp");

    // Write all requests then close stdin
    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        for req in requests {
            serde_json::to_writer(&mut *stdin, req).expect("write request");
            writeln!(stdin).expect("write newline");
        }
    }
    // stdin dropped here → child reads EOF

    let output = child.wait_with_output().expect("wait for child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let val: serde_json::Value = serde_json::from_str(line).ok()?;
            // Only keep JSON-RPC 2.0 responses (skip the CLI envelope)
            if val.get("jsonrpc").is_some() {
                Some(val)
            } else {
                None
            }
        })
        .collect()
}

fn init_home() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let bin = env!("CARGO_BIN_EXE_shiro");
    let output = Command::new(bin)
        .args(["--home", tmp.path().to_str().unwrap()])
        .args(["--log-level", "silent"])
        .arg("init")
        .output()
        .expect("init failed");
    assert!(output.status.success(), "init: {:?}", output);
    tmp
}

// ---------------------------------------------------------------------------
// Initialize
// ---------------------------------------------------------------------------

#[test]
fn mcp_initialize_returns_protocol_version() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        })],
    );

    assert_eq!(responses.len(), 1);
    let r = &responses[0];
    assert_eq!(r["jsonrpc"], "2.0");
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert_eq!(r["result"]["serverInfo"]["name"], "shiro");
}

// ---------------------------------------------------------------------------
// tools/list
// ---------------------------------------------------------------------------

#[test]
fn mcp_tools_list_returns_two_tools() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        ],
    );

    // Skip initialize response
    let r = &responses[1];
    let tools = r["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 2, "exactly two Code Mode tools");

    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"shiro.search"), "missing shiro.search");
    assert!(names.contains(&"shiro.execute"), "missing shiro.execute");

    // Both must have inputSchema
    for tool in tools {
        assert!(
            tool["inputSchema"].is_object(),
            "tool {} missing inputSchema",
            tool["name"]
        );
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
}

// ---------------------------------------------------------------------------
// shiro.search
// ---------------------------------------------------------------------------

#[test]
fn mcp_search_returns_all_ops_on_empty_query() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.search",
                "arguments": {"query": ""}
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], false);
    let content_text = r["result"]["content"][0]["text"].as_str().unwrap();
    let results: serde_json::Value = serde_json::from_str(content_text).unwrap();
    let arr = results.as_array().unwrap();
    assert_eq!(arr.len(), 10, "all 10 SDK operations returned");
}

#[test]
fn mcp_search_deterministic_ordering() {
    let tmp = init_home();

    // Run the same query twice
    let run = |id: u32| {
        let responses = mcp_roundtrip(
            tmp.path(),
            &[
                serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
                serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                    "name": "shiro.search",
                    "arguments": {"query": "document"}
                }}),
            ],
        );
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let r1 = run(2);
    let r2 = run(3);
    assert_eq!(r1, r2, "search results must be deterministic across runs");
}

#[test]
fn mcp_search_unknown_field_rejected() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.search",
                "arguments": {"query": "test", "bogus": true}
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("unknown parameter"), "got: {text}");
}

// ---------------------------------------------------------------------------
// shiro.execute
// ---------------------------------------------------------------------------

#[test]
fn mcp_execute_list_operation() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.execute",
                "arguments": {
                    "program": [
                        {"type": "let", "name": "docs", "call": {"op": "list", "params": {"limit": 5}}},
                        {"type": "return", "value": "$docs"}
                    ]
                }
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], false, "execute failed: {r}");
    let content_text = r["result"]["content"][0]["text"].as_str().unwrap();
    let result: serde_json::Value = serde_json::from_str(content_text).unwrap();
    assert!(result["value"].is_object(), "return value should be object");
    assert!(result["value"]["documents"].is_array());
    assert!(result["steps_executed"].as_u64().unwrap() >= 2);
    assert!(result["trace"].is_array());
}

#[test]
fn mcp_execute_trace_has_correct_structure() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.execute",
                "arguments": {
                    "program": [
                        {"type": "call", "op": "list", "params": {"limit": 1}}
                    ]
                }
            }}),
        ],
    );

    let r = &responses[1];
    let content_text = r["result"]["content"][0]["text"].as_str().unwrap();
    let result: serde_json::Value = serde_json::from_str(content_text).unwrap();
    let trace = result["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["node_type"], "call");
    assert_eq!(trace[0]["op"], "list");
    assert!(trace[0]["args_hash"].is_string());
    assert!(trace[0]["duration_us"].is_number());
}

#[test]
fn mcp_execute_missing_program_error() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.execute",
                "arguments": {}
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("missing required parameter"), "got: {text}");
}

#[test]
fn mcp_execute_unknown_field_rejected() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.execute",
                "arguments": {"program": [], "extra_field": 42}
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], true);
}

#[test]
fn mcp_execute_dsl_unknown_node_field_rejected() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.execute",
                "arguments": {
                    "program": [{"type": "call", "op": "list", "params": {}, "injected": true}]
                }
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("E_DSL_ERROR"), "got: {text}");
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

#[test]
fn mcp_execute_unknown_op_maps_to_error_code() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "shiro.execute",
                "arguments": {
                    "program": [{"type": "call", "op": "nonexistent", "params": {}}]
                }
            }}),
        ],
    );

    let r = &responses[1];
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("E_INVALID_INPUT"), "got: {text}");
}

#[test]
fn mcp_unknown_tool_returns_error() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": "nonexistent.tool",
                "arguments": {}
            }}),
        ],
    );

    let r = &responses[1];
    assert!(r["error"].is_object(), "expected JSON-RPC error");
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn mcp_unknown_method_returns_error() {
    let tmp = init_home();
    let responses = mcp_roundtrip(
        tmp.path(),
        &[serde_json::json!({"jsonrpc":"2.0","id":1,"method":"nonexistent/method","params":{}})],
    );

    let r = &responses[0];
    assert!(r["error"].is_object());
    assert_eq!(r["error"]["code"], -32601);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn mcp_execute_trace_deterministic() {
    let tmp = init_home();

    let run = || {
        let responses = mcp_roundtrip(
            tmp.path(),
            &[
                serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
                serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                    "name": "shiro.execute",
                    "arguments": {
                        "program": [
                            {"type": "let", "name": "docs", "call": {"op": "list", "params": {"limit": 5}}},
                            {"type": "return", "value": "$docs.documents"}
                        ]
                    }
                }}),
            ],
        );
        let text = responses[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let result: serde_json::Value = serde_json::from_str(&text).unwrap();
        // Compare value and trace structure (skip timing fields)
        (
            result["value"].clone(),
            result["steps_executed"].clone(),
            result["trace"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "node_type": t["node_type"],
                        "op": t["op"],
                        "args_hash": t["args_hash"],
                    })
                })
                .collect::<Vec<_>>(),
        )
    };

    let (v1, s1, t1) = run();
    let (v2, s2, t2) = run();
    assert_eq!(v1, v2, "return values differ");
    assert_eq!(s1, s2, "step counts differ");
    assert_eq!(t1, t2, "traces differ (excluding timing)");
}
