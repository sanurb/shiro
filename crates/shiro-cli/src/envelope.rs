//! JSON envelope layer per `docs/CLI.md`.
//!
//! Success: `{ ok, command, result, next_actions }`
//! Error:   `{ ok, command, error: { code, message }, fix?, next_actions }`

use std::collections::BTreeMap;

use serde::Serialize;
use shiro_core::error::{ErrorCode, ShiroError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Output from a successful command execution.
#[derive(Debug)]
pub struct CmdOutput {
    pub result: serde_json::Value,
    pub next_actions: Vec<NextAction>,
}

/// A HATEOAS next-action template per `docs/CLI.md`.
#[derive(Debug, Clone, Serialize)]
pub struct NextAction {
    pub command: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, ParamMeta>>,
}

impl NextAction {
    /// Create a simple next action without params.
    pub fn simple(command: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            description: description.into(),
            params: None,
        }
    }

    /// Create a next action with typed params.
    pub fn with_params(
        command: impl Into<String>,
        description: impl Into<String>,
        params: BTreeMap<String, ParamMeta>,
    ) -> Self {
        Self {
            command: command.into(),
            description: description.into(),
            params: Some(params),
        }
    }
}

/// Typed parameter metadata for next-action templates.
#[derive(Debug, Clone, Serialize)]
pub struct ParamMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Envelope structs (private, serialization-only)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SuccessEnvelope<'a> {
    ok: bool,
    command: &'a str,
    result: &'a serde_json::Value,
    next_actions: &'a [NextAction],
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    ok: bool,
    command: &'a str,
    error: ErrorDetail<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<&'a str>,
    next_actions: &'a [NextAction],
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}

// ---------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------

/// Print a success envelope to stdout. Returns exit code 0.
pub fn print_success(command: &str, output: &CmdOutput) -> i32 {
    let envelope = SuccessEnvelope {
        ok: true,
        command,
        result: &output.result,
        next_actions: &output.next_actions,
    };
    match serde_json::to_string(&envelope) {
        Ok(s) => println!("{s}"),
        Err(e) => {
            tracing::error!("failed to serialize success envelope: {e}");
            print_fallback_error();
            return 1;
        }
    }
    0
}

/// Print an error envelope to stdout. Returns the appropriate exit code.
pub fn print_error(
    command: &str,
    err: &ShiroError,
    fix: Option<&str>,
    next_actions: &[NextAction],
) -> i32 {
    let code = ErrorCode::from_error(err);
    let code_str = code.as_str();
    let message = err.to_string();

    let envelope = ErrorEnvelope {
        ok: false,
        command,
        error: ErrorDetail {
            code: code_str,
            message: &message,
        },
        fix,
        next_actions,
    };
    match serde_json::to_string(&envelope) {
        Ok(s) => println!("{s}"),
        Err(e) => {
            tracing::error!("failed to serialize error envelope: {e}");
            print_fallback_error();
        }
    }

    code.exit_code()
}

fn print_fallback_error() {
    println!(
        r#"{{"ok":false,"command":"unknown","error":{{"code":"E_IO","message":"serialization failed"}},"next_actions":[]}}"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_envelope_shape() {
        let output = CmdOutput {
            result: serde_json::json!({"value": 42}),
            next_actions: vec![NextAction::simple("shiro next", "Do next")],
        };

        let envelope = SuccessEnvelope {
            ok: true,
            command: "shiro test",
            result: &output.result,
            next_actions: &output.next_actions,
        };

        let json_str = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert!(v["ok"].as_bool().unwrap());
        assert_eq!(v["command"].as_str().unwrap(), "shiro test");
        assert!(v["result"].is_object());
        assert_eq!(v["result"]["value"].as_i64().unwrap(), 42);
        assert!(v["next_actions"].is_array());
        assert_eq!(
            v["next_actions"][0]["command"].as_str().unwrap(),
            "shiro next"
        );
        // CLI.md contract: no schemaVersion field.
        assert!(v.get("schemaVersion").is_none());
        assert!(v.get("schema_version").is_none());
    }

    #[test]
    fn error_envelope_shape() {
        let envelope = ErrorEnvelope {
            ok: false,
            command: "shiro test",
            error: ErrorDetail {
                code: "E_PARSE_PDF",
                message: "bad input",
            },
            fix: Some("check your file"),
            next_actions: &[],
        };

        let json_str = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert!(!v["ok"].as_bool().unwrap());
        assert_eq!(v["error"]["code"].as_str().unwrap(), "E_PARSE_PDF");
        assert_eq!(v["error"]["message"].as_str().unwrap(), "bad input");
        assert_eq!(v["fix"].as_str().unwrap(), "check your file");
        assert!(v["next_actions"].is_array());
    }

    #[test]
    fn next_action_with_params() {
        let mut params = BTreeMap::new();
        params.insert(
            "doc_id".to_string(),
            ParamMeta {
                value: Some(serde_json::json!("doc_abc")),
                default: None,
                description: Some("Document ID".to_string()),
            },
        );
        let action = NextAction::with_params("shiro read <doc_id>", "Read document", params);

        let json_str = serde_json::to_string(&action).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["params"]["doc_id"]["value"].as_str().unwrap(), "doc_abc");
        assert_eq!(
            v["params"]["doc_id"]["description"].as_str().unwrap(),
            "Document ID"
        );
    }

    /// Golden test: success envelope MUST have exactly these top-level keys
    /// per CLI.md. Any addition/removal is a contract break.
    #[test]
    fn golden_success_envelope_fields() {
        let output = CmdOutput {
            result: serde_json::json!({"status": "ok"}),
            next_actions: vec![NextAction::simple("shiro doctor", "Check health")],
        };

        let envelope = SuccessEnvelope {
            ok: true,
            command: "shiro init",
            result: &output.result,
            next_actions: &output.next_actions,
        };

        let json_str = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let obj = v.as_object().unwrap();

        // Contract: exactly these keys, no more, no fewer.
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            &["command", "next_actions", "ok", "result"],
            "success envelope keys must match CLI.md contract exactly"
        );
        assert_eq!(obj.len(), 4);

        // ok must be true.
        assert_eq!(obj["ok"], serde_json::json!(true));
        // command must be a string.
        assert!(obj["command"].is_string());
        // result must be an object.
        assert!(obj["result"].is_object());
        // next_actions must be an array.
        assert!(obj["next_actions"].is_array());
    }

    /// Golden test: error envelope MUST have exactly these top-level keys
    /// per CLI.md. `fix` is optional (skip_serializing_if).
    #[test]
    fn golden_error_envelope_fields() {
        // With fix
        let envelope_with_fix = ErrorEnvelope {
            ok: false,
            command: "shiro search",
            error: ErrorDetail {
                code: "E_STORE_CORRUPT",
                message: "db damaged",
            },
            fix: Some("run doctor to diagnose"),
            next_actions: &[],
        };
        let json_str = serde_json::to_string(&envelope_with_fix).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            &["command", "error", "fix", "next_actions", "ok"],
            "error envelope with fix must have these keys"
        );
        assert_eq!(obj.len(), 5);

        // Without fix
        let envelope_no_fix = ErrorEnvelope {
            ok: false,
            command: "shiro search",
            error: ErrorDetail {
                code: "E_SEARCH_FAILED",
                message: "bad query",
            },
            fix: None,
            next_actions: &[],
        };
        let json_str = serde_json::to_string(&envelope_no_fix).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            &["command", "error", "next_actions", "ok"],
            "error envelope without fix must omit fix key"
        );
        assert_eq!(obj.len(), 4);

        // error sub-object must have exactly code + message.
        let mut err_keys: Vec<&str> = v["error"]
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        err_keys.sort_unstable();
        assert_eq!(err_keys, &["code", "message"]);
    }
}
