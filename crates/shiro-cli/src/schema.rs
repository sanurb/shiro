#![allow(dead_code)]

//! Typed result schemas for CLI command outputs.
//!
//! These structs define the expected JSON shape of each command's `result` field.
//! They serve as living documentation and schema contract enforcement in tests.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Result of `shiro init`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct InitResult {
    pub created: bool,
    pub home: String,
}

/// Result of `shiro add`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AddResult {
    pub doc_id: String,
    pub status: String,
    pub title: String,
    pub segments: u64,
    pub changed: bool,
}

/// Result of `shiro ingest`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct IngestResult {
    pub added: u64,
    pub ready: u64,
    pub failed: u64,
    pub failures: Vec<IngestFailure>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct IngestFailure {
    pub source: String,
    pub code: String,
    pub message: String,
}

/// A single search hit (EntryPoint shape per ADR-007).
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    pub result_id: String,
    pub doc_id: String,
    pub block_idx: u64,
    pub block_kind: String,
    pub span: SpanResult,
    pub snippet: String,
    pub scores: ScoresResult,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SpanResult {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ScoresResult {
    pub bm25: Bm25Score,
    pub fused: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Bm25Score {
    pub score: f64,
    pub rank: u64,
}

/// Result of `shiro search`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub query: String,
    pub mode: String,
    pub results: Vec<SearchHit>,
}

/// Result of `shiro capabilities`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CapabilitiesResult {
    pub version: String,
    pub schema_version: u32,
    pub state_machine: StateMachineInfo,
    pub id_schemes: BTreeMap<String, IdSchemeInfo>,
    pub parsers: Vec<String>,
    pub features: BTreeMap<String, bool>,
    pub storage: StorageInfo,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct StateMachineInfo {
    pub states: Vec<String>,
    pub transitions: Vec<TransitionInfo>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct TransitionInfo {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct IdSchemeInfo {
    pub prefix: String,
    pub algorithm: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct StorageInfo {
    pub backend: String,
    pub wal_mode: bool,
}

/// Result of `shiro doctor`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DoctorResult {
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that CapabilitiesResult can round-trip through JSON.
    #[test]
    fn capabilities_schema_roundtrip() {
        let cap = CapabilitiesResult {
            version: "0.3.0".to_string(),
            schema_version: 1,
            state_machine: StateMachineInfo {
                states: vec!["STAGED".into(), "READY".into()],
                transitions: vec![TransitionInfo {
                    from: "STAGED".into(),
                    to: "READY".into(),
                }],
            },
            id_schemes: BTreeMap::from([(
                "doc_id".into(),
                IdSchemeInfo {
                    prefix: "doc_".into(),
                    algorithm: "blake3".into(),
                },
            )]),
            parsers: vec!["plaintext".into()],
            features: BTreeMap::from([("fts_bm25".into(), true)]),
            storage: StorageInfo {
                backend: "sqlite".into(),
                wal_mode: true,
            },
        };
        let json = serde_json::to_string(&cap).unwrap();
        let back: CapabilitiesResult = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    /// Verify AddResult shape.
    #[test]
    fn add_result_roundtrip() {
        let add = AddResult {
            doc_id: "doc_abc".into(),
            status: "READY".into(),
            title: "Test".into(),
            segments: 3,
            changed: true,
        };
        let json = serde_json::to_string(&add).unwrap();
        let back: AddResult = serde_json::from_str(&json).unwrap();
        assert_eq!(add, back);
    }
}
