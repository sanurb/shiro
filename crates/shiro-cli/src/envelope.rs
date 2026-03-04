use std::fmt;

use serde::Serialize;
use shiro_core::error::{ErrorCode, ShiroError};

pub const SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize)]
pub struct NextAction {
    pub command: String,
    pub description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SuccessJson<'a, T: Serialize> {
    schema_version: &'static str,
    ok: bool,
    command: &'a str,
    data: &'a T,
    next_actions: &'a [NextAction],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorJson<'a> {
    schema_version: &'static str,
    ok: bool,
    command: &'a str,
    error: ErrorDetailJson<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<&'a str>,
    next_actions: &'a [NextAction],
}

#[derive(Serialize)]
struct ErrorDetailJson<'a> {
    code: &'a str,
    message: &'a str,
}

/// Print a success envelope. Returns exit code.
pub fn print_success<T: Serialize + fmt::Display>(
    command: &str,
    data: &T,
    next_actions: &[NextAction],
    json: bool,
) -> i32 {
    if json {
        let envelope = SuccessJson {
            schema_version: SCHEMA_VERSION,
            ok: true,
            command,
            data,
            next_actions,
        };
        match serde_json::to_string(&envelope) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                tracing::error!("failed to serialize success envelope: {e}");
                println!(
                    r#"{{"schemaVersion":"1.0","ok":false,"command":"unknown","error":{{"code":"internal","message":"serialization failed"}}}}"#
                );
                return 5;
            }
        }
        0
    } else {
        println!("{data}");
        0
    }
}

/// Print an error envelope from a ShiroError. Returns exit code.
pub fn print_error(
    command: &str,
    err: &ShiroError,
    fix: Option<&str>,
    next_actions: &[NextAction],
    json: bool,
) -> i32 {
    let code = ErrorCode::from_error(err);
    let code_str = code.as_str();
    let message = err.to_string();

    if json {
        let envelope = ErrorJson {
            schema_version: SCHEMA_VERSION,
            ok: false,
            command,
            error: ErrorDetailJson {
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
                println!(
                    r#"{{"schemaVersion":"1.0","ok":false,"command":"unknown","error":{{"code":"internal","message":"serialization failed"}}}}"#
                );
            }
        }
        code.exit_code()
    } else {
        eprintln!("error [{code_str}]: {message}");
        if let Some(f) = fix {
            eprintln!("fix: {f}");
        }
        code.exit_code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_envelope_shape() {
        #[derive(Serialize)]
        struct TestData {
            value: i32,
        }

        let envelope = SuccessJson {
            schema_version: SCHEMA_VERSION,
            ok: true,
            command: "test",
            data: &TestData { value: 42 },
            next_actions: &[NextAction {
                command: "next".into(),
                description: "do next".into(),
            }],
        };

        let json_str = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["schemaVersion"].as_str().unwrap(), "1.0");
        assert!(v["ok"].as_bool().unwrap());
        assert_eq!(v["command"].as_str().unwrap(), "test");
        assert!(v["data"].is_object());
        assert_eq!(v["data"]["value"].as_i64().unwrap(), 42);
        assert!(v["nextActions"].is_array());
        assert_eq!(v["nextActions"][0]["command"].as_str().unwrap(), "next");
    }

    #[test]
    fn test_error_envelope_shape() {
        let envelope = ErrorJson {
            schema_version: SCHEMA_VERSION,
            ok: false,
            command: "test",
            error: ErrorDetailJson {
                code: "parse_failed",
                message: "bad input",
            },
            fix: Some("check your input"),
            next_actions: &[],
        };

        let json_str = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["schemaVersion"].as_str().unwrap(), "1.0");
        assert!(!v["ok"].as_bool().unwrap());
        assert_eq!(v["command"].as_str().unwrap(), "test");
        assert!(v["error"].is_object());
        assert_eq!(v["error"]["code"].as_str().unwrap(), "parse_failed");
        assert_eq!(v["error"]["message"].as_str().unwrap(), "bad input");
        assert_eq!(v["fix"].as_str().unwrap(), "check your input");
        assert!(v["nextActions"].is_array());
    }

    #[test]
    fn test_schema_version_is_stable() {
        assert_eq!(SCHEMA_VERSION, "1.0");
    }
}
