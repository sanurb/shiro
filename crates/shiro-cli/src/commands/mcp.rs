//! `shiro mcp` — Code Mode MCP (Model Context Protocol) JSON-RPC 2.0 server.
//!
//! Exposes exactly two tools:
//! - `shiro.search` — query the SDK spec registry to discover operations.
//! - `shiro.execute` — run a DSL program (JSON AST) against the SDK.
//!
//! Transport: newline-delimited JSON over stdio.
//! Protocol version: 2024-11-05.

use crate::envelope::CmdOutput;
use shiro_core::{ErrorCode, ShiroError};
use shiro_index::FtsIndex;
use shiro_parse::MarkdownParser;
use shiro_store::Store;
use std::io::{self, BufRead, Write};

/// Entry point for `shiro mcp`.
pub fn run(home: shiro_core::ShiroHome) -> Result<CmdOutput, ShiroError> {
    run_server(home)?;
    Ok(CmdOutput {
        result: serde_json::json!({"status": "stopped"}),
        next_actions: vec![],
    })
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

fn run_server(home: shiro_core::ShiroHome) -> Result<(), ShiroError> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    // Lazily initialized on first execute call.
    let mut ctx: Option<ServerCtx> = None;

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| ShiroError::McpError {
            message: format!("stdin read: {e}"),
        })?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| ShiroError::McpError {
                message: format!("invalid JSON-RPC: {e}"),
            })?;

        let id = request
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let response = match method {
            "initialize" => handle_initialize(id),
            "notifications/initialized" => continue, // notification — no response
            "tools/list" => handle_tools_list(id),
            "tools/call" => {
                let params = request.get("params");
                handle_tools_call(id, params, &mut ctx, &home)
            }
            _ => jsonrpc_error(id, -32601, &format!("method not found: {method}")),
        };

        let mut out = stdout.lock();
        serde_json::to_writer(&mut out, &response).map_err(|e| ShiroError::McpError {
            message: format!("stdout write: {e}"),
        })?;
        writeln!(out).map_err(|e| ShiroError::McpError {
            message: format!("stdout newline: {e}"),
        })?;
        out.flush().map_err(|e| ShiroError::McpError {
            message: format!("stdout flush: {e}"),
        })?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Method handlers
// ---------------------------------------------------------------------------

fn handle_initialize(id: serde_json::Value) -> serde_json::Value {
    jsonrpc_ok(
        id,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "shiro",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }),
    )
}

fn handle_tools_list(id: serde_json::Value) -> serde_json::Value {
    jsonrpc_ok(id, serde_json::json!({ "tools": tools_list() }))
}

fn handle_tools_call(
    id: serde_json::Value,
    params: Option<&serde_json::Value>,
    ctx: &mut Option<ServerCtx>,
    home: &shiro_core::ShiroHome,
) -> serde_json::Value {
    let tool_name = params
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(serde_json::json!({}));

    match tool_name {
        "shiro.search" => handle_search(id, &arguments),
        "shiro.execute" => handle_execute(id, &arguments, ctx, home),
        _ => jsonrpc_error(id, -32602, &format!("unknown tool: {tool_name}")),
    }
}

// ---------------------------------------------------------------------------
// Tool: shiro.search
// ---------------------------------------------------------------------------

fn handle_search(id: serde_json::Value, arguments: &serde_json::Value) -> serde_json::Value {
    // Validate: query is required string
    let query = match arguments.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return tool_error(
                id,
                "E_INVALID_INPUT",
                "missing required parameter: 'query' (string)",
            );
        }
    };

    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    // Reject unknown fields
    if let Some(obj) = arguments.as_object() {
        for key in obj.keys() {
            if key != "query" && key != "limit" {
                return tool_error(
                    id,
                    "E_INVALID_INPUT",
                    &format!("unknown parameter: '{key}'"),
                );
            }
        }
    }

    let results = shiro_sdk::spec::search_specs(query, limit);
    let serialized = serde_json::to_value(&results).unwrap_or_default();

    tool_ok(id, &serialized.to_string())
}

// ---------------------------------------------------------------------------
// Tool: shiro.execute
// ---------------------------------------------------------------------------

fn handle_execute(
    id: serde_json::Value,
    arguments: &serde_json::Value,
    ctx: &mut Option<ServerCtx>,
    home: &shiro_core::ShiroHome,
) -> serde_json::Value {
    // Validate: program is required
    let program = match arguments.get("program") {
        Some(p) => p,
        None => {
            return tool_error(
                id,
                "E_INVALID_INPUT",
                "missing required parameter: 'program' (array of DSL nodes)",
            );
        }
    };

    if !program.is_array() {
        return tool_error(
            id,
            "E_INVALID_INPUT",
            "'program' must be a JSON array of DSL nodes",
        );
    }

    // Parse optional limits
    let limits = match arguments.get("limits") {
        Some(l) => match serde_json::from_value::<shiro_sdk::dsl::Limits>(l.clone()) {
            Ok(limits) => limits,
            Err(e) => {
                return tool_error(id, "E_INVALID_INPUT", &format!("invalid 'limits': {e}"));
            }
        },
        None => shiro_sdk::dsl::Limits::default(),
    };

    // Reject unknown fields
    if let Some(obj) = arguments.as_object() {
        for key in obj.keys() {
            if key != "program" && key != "limits" {
                return tool_error(
                    id,
                    "E_INVALID_INPUT",
                    &format!("unknown parameter: '{key}'"),
                );
            }
        }
    }

    // Ensure server context is initialized
    let c = match ensure_ctx(ctx, home) {
        Ok(c) => c,
        Err(e) => {
            let code = ErrorCode::from_error(&e);
            return tool_error(id, code.as_str(), &format!("init failed: {e}"));
        }
    };

    let parser = MarkdownParser;
    match shiro_sdk::dsl::execute_program(&c.home, &c.store, &c.fts, &parser, program, limits) {
        Ok(result) => {
            let serialized = serde_json::to_value(&result).unwrap_or_default();
            tool_ok(id, &serialized.to_string())
        }
        Err(e) => {
            let code = ErrorCode::from_error(&e);
            tool_error(id, code.as_str(), &e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn tools_list() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "shiro.search",
            "description": "Search the SDK spec registry to discover available operations, their parameters, schemas, and examples. Use this first to understand what operations are available.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword to search for in operation names and descriptions. Empty string returns all operations."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 10).",
                        "minimum": 1,
                        "maximum": 100
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        },
        {
            "name": "shiro.execute",
            "description": "Execute a DSL program against the SDK. Programs are JSON arrays of typed nodes: let, call, if, for_each, return. Use 'shiro.search' first to discover available operations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "program": {
                        "type": "array",
                        "description": "Array of DSL nodes to execute. Each node has a 'type' field: 'let', 'call', 'if', 'for_each', or 'return'.",
                        "items": {
                            "type": "object"
                        }
                    },
                    "limits": {
                        "type": "object",
                        "description": "Optional execution limits (defaults: max_steps=200, max_iterations=100, max_output_bytes=1048576, timeout_ms=30000).",
                        "properties": {
                            "max_steps": { "type": "integer", "minimum": 1 },
                            "max_iterations": { "type": "integer", "minimum": 1 },
                            "max_output_bytes": { "type": "integer", "minimum": 1 },
                            "timeout_ms": { "type": "integer", "minimum": 1 }
                        },
                        "additionalProperties": false
                    }
                },
                "required": ["program"],
                "additionalProperties": false
            }
        }
    ])
}

// ---------------------------------------------------------------------------
// Lazy server context (home/store/fts)
// ---------------------------------------------------------------------------

struct ServerCtx {
    home: shiro_core::ShiroHome,
    store: Store,
    fts: FtsIndex,
}

impl ServerCtx {
    fn init(home: &shiro_core::ShiroHome) -> Result<Self, ShiroError> {
        let store = Store::open(&home.db_path())?;
        let fts = FtsIndex::open(&home.tantivy_dir())?;
        Ok(Self {
            home: home.clone(),
            store,
            fts,
        })
    }
}

fn ensure_ctx<'a>(
    ctx: &'a mut Option<ServerCtx>,
    home: &shiro_core::ShiroHome,
) -> Result<&'a ServerCtx, ShiroError> {
    if ctx.is_none() {
        *ctx = Some(ServerCtx::init(home)?);
    }
    Ok(ctx.as_ref().expect("ctx must be Some after init"))
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 helpers
// ---------------------------------------------------------------------------

fn jsonrpc_ok(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: serde_json::Value, code: i32, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// MCP tool success response (content array with text).
fn tool_ok(id: serde_json::Value, text: &str) -> serde_json::Value {
    jsonrpc_ok(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "isError": false,
        }),
    )
}

/// MCP tool error response (content array with error text, stable error code).
fn tool_error(id: serde_json::Value, code: &str, message: &str) -> serde_json::Value {
    jsonrpc_ok(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": format!("{code}: {message}")}],
            "isError": true,
        }),
    )
}
