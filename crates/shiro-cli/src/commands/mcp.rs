//! `shiro mcp` — Code Mode MCP (Model Context Protocol) JSON-RPC 2.0 server.
//!
//! Exposes exactly two tools:
//! - `shiro.search` — query the SDK spec registry to discover operations.
//! - `shiro.execute` — run a DSL program (JSON AST) against the SDK.
//!
//! Transport: newline-delimited JSON over stdio.
//! Protocol versions: 2026-07-28 (modern) plus legacy handshake compatibility.

use crate::envelope::CmdOutput;
use shiro_core::{ErrorCode, ShiroError};
use shiro_parse::MarkdownParser;
use shiro_sdk::executor::ExecutorPorts;
use std::io::{self, BufRead, Write};

/// Entry point for `shiro mcp`.
pub fn run(home: shiro_core::ShiroHome, allow_writes: bool) -> Result<CmdOutput, ShiroError> {
    run_server(home, allow_writes)?;
    Ok(CmdOutput {
        result: serde_json::json!({"status": "stopped"}),
        next_actions: vec![],
    })
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

fn run_server(home: shiro_core::ShiroHome, allow_writes: bool) -> Result<(), ShiroError> {
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

        let response = if let Some(error) = validate_modern_request(&request, &id) {
            error
        } else {
            match method {
                "server/discover" => handle_discover(id),
                "initialize" => handle_initialize(id, request.get("params")),
                "notifications/initialized" => continue, // notification — no response
                "tools/list" => handle_tools_list(id),
                "tools/call" => {
                    let params = request.get("params");
                    handle_tools_call(id, params, &mut ctx, &home, allow_writes)
                }
                _ => jsonrpc_error(id, -32601, &format!("method not found: {method}")),
            }
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

fn validate_modern_request(
    request: &serde_json::Value,
    id: &serde_json::Value,
) -> Option<serde_json::Value> {
    let metadata = request.get("params").and_then(|params| params.get("_meta"));
    let metadata = metadata?;
    let requested = metadata
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(serde_json::Value::as_str);
    let requested = requested?;
    if requested != "2026-07-28" {
        return Some(jsonrpc_error_data(
            id.clone(),
            -32022,
            "Unsupported protocol version",
            serde_json::json!({
                "supported": ["2026-07-28", "2025-11-25", "2025-06-18", "2024-11-05"],
                "requested": requested,
            }),
        ));
    }
    if metadata
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_none()
    {
        return Some(jsonrpc_error(
            id.clone(),
            -32602,
            "modern MCP requests require clientCapabilities in _meta",
        ));
    }
    None
}

fn handle_discover(id: serde_json::Value) -> serde_json::Value {
    jsonrpc_ok(
        id,
        serde_json::json!({
            "supportedVersions": ["2026-07-28", "2025-11-25", "2025-06-18", "2024-11-05"],
            "capabilities": { "tools": {} },
            "instructions": "Use shiro.search for operation discovery, then shiro.execute. Writes require explicit host, actor, and approval authority.",
            "cacheScope": "public",
            "_meta": {
                "io.modelcontextprotocol/serverInfo": {
                    "name": "shiro",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
    )
}

fn handle_initialize(
    id: serde_json::Value,
    params: Option<&serde_json::Value>,
) -> serde_json::Value {
    let requested = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(serde_json::Value::as_str);
    let protocol_version = match requested {
        Some("2024-11-05") => "2024-11-05",
        Some("2025-06-18") => "2025-06-18",
        _ => "2025-11-25",
    };
    jsonrpc_ok(
        id,
        serde_json::json!({
            "protocolVersion": protocol_version,
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
    allow_writes: bool,
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
        "shiro.execute" => handle_execute(id, &arguments, ctx, home, allow_writes),
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

    tool_ok(
        id,
        &serialized.to_string(),
        serde_json::json!({"results": serialized}),
    )
}

// ---------------------------------------------------------------------------
// Tool: shiro.execute
// ---------------------------------------------------------------------------

fn handle_execute(
    id: serde_json::Value,
    arguments: &serde_json::Value,
    ctx: &mut Option<ServerCtx>,
    home: &shiro_core::ShiroHome,
    allow_writes: bool,
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

    let actor_id = arguments.get("actor_id").and_then(|value| value.as_str());
    let approval_id = arguments
        .get("approval_id")
        .and_then(|value| value.as_str());
    let run_id = shiro_core::RunId::generate();

    // Reject unknown fields
    if let Some(obj) = arguments.as_object() {
        for key in obj.keys() {
            if key != "program" && key != "limits" && key != "actor_id" && key != "approval_id" {
                return tool_error(
                    id,
                    "E_INVALID_INPUT",
                    &format!("unknown parameter: '{key}'"),
                );
            }
        }
    }

    let server_ctx = match ensure_ctx(ctx, home) {
        Ok(server_ctx) => server_ctx,
        Err(e) => {
            let code = ErrorCode::from_error(&e);
            return tool_error(id, code.as_str(), &format!("init failed: {e}"));
        }
    };

    // Select adapters after DSL control flow and variable substitution resolve
    // each call. An unexecuted vector branch or resolved BM25 mode therefore
    // cannot inherit failures from an unused embedding provider.
    let parser = MarkdownParser;
    let mut call_handler = |op: &str, params: &serde_json::Value| {
        let is_write = shiro_sdk::spec::operation_authority(op) == "write";
        if is_write && !allow_writes {
            return Err(ShiroError::McpError {
                message: format!(
                    "write operation '{op}' requires host startup with --allow-writes"
                ),
            });
        }
        let actor = if is_write {
            Some(
                actor_id
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| ShiroError::McpError {
                        message: format!("write operation '{op}' requires actor_id"),
                    })?,
            )
        } else {
            None
        };
        let approval = if is_write {
            Some(
                approval_id
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| ShiroError::McpError {
                        message: format!("write operation '{op}' requires approval_id"),
                    })?,
            )
        } else {
            None
        };
        let profile = runtime_profile_for_call(op, params);
        let engine = server_ctx.engine(profile)?;
        let ports = ExecutorPorts {
            embedder: engine.embedder(),
            vector_index: engine.vector_index(),
            reranker: engine.reranker(),
        };
        let call = serde_json::json!({ "op": op, "params": params });
        let params_digest = blake3::hash(params.to_string().as_bytes())
            .to_hex()
            .to_string();
        if let (Some(actor), Some(approval)) = (actor, approval) {
            engine.store.record_mcp_mutation(
                run_id.as_str(),
                actor,
                approval,
                op,
                &params_digest,
                "AUTHORIZED",
            )?;
        }
        let result = shiro_sdk::executor::execute_with_ports(
            &engine.home,
            &engine.store,
            &engine.fts,
            &parser,
            ports,
            &call,
        );
        if let (Some(actor), Some(approval)) = (actor, approval) {
            let outcome = if result.is_ok() {
                "SUCCEEDED"
            } else {
                "FAILED"
            };
            engine.store.record_mcp_mutation(
                run_id.as_str(),
                actor,
                approval,
                op,
                &params_digest,
                outcome,
            )?;
        }
        if is_write && result.is_ok() {
            server_ctx.refresh_after_write()?;
        }
        result
    };
    match shiro_sdk::dsl::execute_program_with_call_handler(program, limits, &mut call_handler) {
        Ok(result) => {
            let serialized = serde_json::to_value(&result).unwrap_or_default();
            tool_ok(
                id,
                &serialized.to_string(),
                serde_json::json!({"result": serialized, "run_id": run_id.as_str()}),
            )
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
            "description": "Search the SDK spec registry to discover available operations, their parameters, schemas, authority, and examples. Use this first to understand what operations are available.",
            "annotations": { "readOnlyHint": true, "idempotentHint": true },
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
            },
            "outputSchema": {
                "type": "object",
                "properties": { "results": { "type": "array" } },
                "required": ["results"],
                "additionalProperties": false
            }
        },
        {
            "name": "shiro.execute",
            "description": "Execute a DSL program against the SDK. Programs are JSON arrays of typed nodes: let, call, if, for_each, return. Write-class operations require host --allow-writes plus actor_id and approval_id.",
            "annotations": { "readOnlyHint": false },
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
                    "actor_id": {
                        "type": "string",
                        "description": "Required actor identity when a write-class operation executes."
                    },
                    "approval_id": {
                        "type": "string",
                        "description": "Required host/policy approval reference when a write-class operation executes."
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
            },
            "outputSchema": {
                "type": "object",
                "properties": { "result": {}, "run_id": { "type": "string" } },
                "required": ["result", "run_id"],
                "additionalProperties": false
            }
        }
    ])
}

// ---------------------------------------------------------------------------
// Lazy server context
// ---------------------------------------------------------------------------

struct ServerCtx {
    home: shiro_core::ShiroHome,
    base_engine: shiro_sdk::Engine,
    rerank_engine: Option<shiro_sdk::Engine>,
    vector_engine: Option<shiro_sdk::Engine>,
    full_engine: Option<shiro_sdk::Engine>,
}

impl ServerCtx {
    fn init(home: &shiro_core::ShiroHome) -> Result<Self, ShiroError> {
        let base_engine = crate::runtime::open_engine(home, crate::runtime::RuntimeProfile::Base)?;
        Ok(Self {
            home: home.clone(),
            base_engine,
            rerank_engine: None,
            vector_engine: None,
            full_engine: None,
        })
    }

    fn refresh_after_write(&mut self) -> Result<(), ShiroError> {
        self.base_engine =
            crate::runtime::open_engine(&self.home, crate::runtime::RuntimeProfile::Base)?;
        self.rerank_engine = None;
        self.vector_engine = None;
        self.full_engine = None;
        Ok(())
    }

    fn engine(
        &mut self,
        profile: crate::runtime::RuntimeProfile,
    ) -> Result<&shiro_sdk::Engine, ShiroError> {
        match profile {
            crate::runtime::RuntimeProfile::Base => Ok(&self.base_engine),
            crate::runtime::RuntimeProfile::RerankOnly => {
                if self.rerank_engine.is_none() {
                    self.rerank_engine = Some(crate::runtime::open_engine(
                        &self.home,
                        crate::runtime::RuntimeProfile::RerankOnly,
                    )?);
                }
                self.rerank_engine
                    .as_ref()
                    .ok_or_else(|| ShiroError::McpError {
                        message: "rerank runtime initialization did not produce an Engine"
                            .to_string(),
                    })
            }
            crate::runtime::RuntimeProfile::Vector => {
                if self.vector_engine.is_none() {
                    self.vector_engine = Some(crate::runtime::open_engine(
                        &self.home,
                        crate::runtime::RuntimeProfile::Vector,
                    )?);
                }
                self.vector_engine
                    .as_ref()
                    .ok_or_else(|| ShiroError::McpError {
                        message: "vector runtime initialization did not produce an Engine"
                            .to_string(),
                    })
            }
            crate::runtime::RuntimeProfile::Full => {
                if self.full_engine.is_none() {
                    self.full_engine = Some(crate::runtime::open_engine(
                        &self.home,
                        crate::runtime::RuntimeProfile::Full,
                    )?);
                }
                self.full_engine
                    .as_ref()
                    .ok_or_else(|| ShiroError::McpError {
                        message: "full runtime initialization did not produce an Engine"
                            .to_string(),
                    })
            }
        }
    }
}

fn ensure_ctx<'a>(
    ctx: &'a mut Option<ServerCtx>,
    home: &shiro_core::ShiroHome,
) -> Result<&'a mut ServerCtx, ShiroError> {
    if ctx.is_none() {
        *ctx = Some(ServerCtx::init(home)?);
    }
    ctx.as_mut().ok_or_else(|| ShiroError::McpError {
        message: "MCP runtime context initialization failed".to_string(),
    })
}

/// Select adapters from one operation after DSL parameters are resolved.
fn runtime_profile_for_call(
    op: &str,
    params: &serde_json::Value,
) -> crate::runtime::RuntimeProfile {
    if op != "search" && op != "search_pack" {
        return crate::runtime::RuntimeProfile::Base;
    }
    let vector = params.get("mode").and_then(|mode| mode.as_str()) != Some("bm25");
    let reranker = params
        .get("rerank")
        .and_then(|rerank| rerank.as_bool())
        .unwrap_or(false);
    match (vector, reranker) {
        (false, false) => crate::runtime::RuntimeProfile::Base,
        (false, true) => crate::runtime::RuntimeProfile::RerankOnly,
        (true, false) => crate::runtime::RuntimeProfile::Vector,
        (true, true) => crate::runtime::RuntimeProfile::Full,
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 helpers
// ---------------------------------------------------------------------------

fn jsonrpc_ok(id: serde_json::Value, mut result: serde_json::Value) -> serde_json::Value {
    if let Some(object) = result.as_object_mut() {
        object
            .entry("resultType")
            .or_insert_with(|| serde_json::Value::String("complete".to_string()));
        object.entry("_meta").or_insert_with(|| {
            serde_json::json!({
                "io.modelcontextprotocol/serverInfo": {
                    "name": "shiro",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })
        });
    }
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

fn jsonrpc_error_data(
    id: serde_json::Value,
    code: i32,
    message: &str,
    data: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message, "data": data },
    })
}

/// MCP tool success response (content array with text).
fn tool_ok(
    id: serde_json::Value,
    text: &str,
    structured_content: serde_json::Value,
) -> serde_json::Value {
    jsonrpc_ok(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured_content,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_profile_uses_resolved_search_parameters() {
        assert_eq!(
            runtime_profile_for_call("list", &serde_json::json!({})),
            crate::runtime::RuntimeProfile::Base
        );
        assert_eq!(
            runtime_profile_for_call("search", &serde_json::json!({ "mode": "bm25" })),
            crate::runtime::RuntimeProfile::Base
        );
        assert_eq!(
            runtime_profile_for_call(
                "search",
                &serde_json::json!({ "mode": "bm25", "rerank": true })
            ),
            crate::runtime::RuntimeProfile::RerankOnly
        );
        assert_eq!(
            runtime_profile_for_call("search", &serde_json::json!({})),
            crate::runtime::RuntimeProfile::Vector
        );
        assert_eq!(
            runtime_profile_for_call("search", &serde_json::json!({ "rerank": true })),
            crate::runtime::RuntimeProfile::Full
        );
    }
}
