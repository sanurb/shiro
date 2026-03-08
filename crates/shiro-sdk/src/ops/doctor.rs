//! `doctor` — consistency checks and diagnostics.

use serde::{Deserialize, Serialize};
use shiro_core::ports::VectorIndex;
use shiro_core::{ShiroError, ShiroHome};
use shiro_embed::FlatIndex;
use shiro_index::FtsIndex;
use shiro_store::Store;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DoctorInput {
    pub verify_vector: bool,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DoctorCheck {
    pub name: String,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DoctorOutput {
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
}

/// Doctor is a diagnostic tool — it opens Store/FtsIndex internally so it can
/// report failures as check results rather than hard errors.
pub fn execute(home: &ShiroHome, input: &DoctorInput) -> Result<DoctorOutput, ShiroError> {
    let mut checks = Vec::new();

    // Check 1: home directory exists.
    let home_exists = home.root().as_std_path().is_dir();
    checks.push(DoctorCheck {
        name: "home_directory".into(),
        status: if home_exists { "ok" } else { "fail" }.into(),
        message: if home_exists {
            format!("{} exists", home.root())
        } else {
            format!("{} not found — run `shiro init`", home.root())
        },
        details: None,
    });

    if !home_exists {
        return Ok(DoctorOutput {
            healthy: false,
            checks,
        });
    }

    // Check 2: SQLite database.
    let mut store_opt: Option<Store> = None;
    let (db_ok, db_check) = match Store::open(&home.db_path()) {
        Ok(store) => {
            let counts = store.count_by_state().unwrap_or_default();
            let total: usize = counts.iter().map(|(_, c)| c).sum();
            let details: Vec<serde_json::Value> = counts
                .iter()
                .map(|(s, c)| serde_json::json!({ "state": s.as_str(), "count": c }))
                .collect();
            store_opt = Some(store);
            (
                true,
                DoctorCheck {
                    name: "sqlite_store".into(),
                    status: "ok".into(),
                    message: format!("{total} documents in store"),
                    details: Some(serde_json::Value::Array(details)),
                },
            )
        }
        Err(e) => (
            false,
            DoctorCheck {
                name: "sqlite_store".into(),
                status: "fail".into(),
                message: format!("cannot open store: {e}"),
                details: None,
            },
        ),
    };
    checks.push(db_check);

    // Check 3: Tantivy FTS index.
    let fts_ok = match FtsIndex::open(&home.tantivy_dir()) {
        Ok(fts) => {
            let count = fts.num_segments().unwrap_or(0);
            checks.push(DoctorCheck {
                name: "fts_index".into(),
                status: "ok".into(),
                message: format!("{count} segments indexed"),
                details: None,
            });
            true
        }
        Err(e) => {
            checks.push(DoctorCheck {
                name: "fts_index".into(),
                status: "fail".into(),
                message: format!("cannot open FTS index: {e}"),
                details: None,
            });
            false
        }
    };

    // Check 4: schema_version
    if let Some(ref store) = store_opt {
        match store.schema_version() {
            Ok(v) => checks.push(DoctorCheck {
                name: "schema_version".into(),
                status: "ok".into(),
                message: format!("schema version {v}"),
                details: None,
            }),
            Err(_) => checks.push(DoctorCheck {
                name: "schema_version".into(),
                status: "warn".into(),
                message: "schema_meta table missing or corrupt".into(),
                details: None,
            }),
        }
    }

    // Check 5: document_states
    if let Some(ref store) = store_opt {
        let counts = store.count_by_state().unwrap_or_default();
        let indexing_count: usize = counts
            .iter()
            .filter(|(s, _)| s.as_str() == "INDEXING")
            .map(|(_, c)| c)
            .sum();
        let details: Vec<serde_json::Value> = counts
            .iter()
            .map(|(s, c)| serde_json::json!({ "state": s.as_str(), "count": c }))
            .collect();
        let (status, message) = if indexing_count > 0 {
            (
                "warn",
                format!("{indexing_count} documents stuck in INDEXING state"),
            )
        } else {
            let total: usize = counts.iter().map(|(_, c)| c).sum();
            ("ok", format!("{total} documents across all states"))
        };
        checks.push(DoctorCheck {
            name: "document_states".into(),
            status: status.into(),
            message,
            details: Some(serde_json::Value::Array(details)),
        });
    }

    // Check 6: processing fingerprints (ADR-004)
    if let Some(ref store) = store_opt {
        let counts = store.count_by_state().unwrap_or_default();
        let ready_count: usize = counts
            .iter()
            .filter(|(s, _)| s.as_str() == "READY")
            .map(|(_, c)| *c)
            .sum();
        if ready_count > 0 {
            let docs = store.list_documents(ready_count).unwrap_or_default();
            let mut missing = 0usize;
            for (doc_id, state, _title) in &docs {
                if state.as_str() == "READY" {
                    if let Ok(None) = store.get_fingerprint(doc_id) {
                        missing += 1;
                    }
                }
            }
            let (status, message) = if missing > 0 {
                (
                    "warn",
                    format!("{missing} READY documents missing processing fingerprint — run `shiro reindex` to reprocess"),
                )
            } else {
                ("ok", format!("{ready_count} READY documents have processing fingerprints"))
            };
            checks.push(DoctorCheck {
                name: "processing_fingerprints".into(),
                status: status.into(),
                message,
                details: None,
            });
        }
    }

    // Check 7: FTS consistency
    if let (Some(ref store), true) = (&store_opt, fts_ok) {
        let counts = store.count_by_state().unwrap_or_default();
        let ready_count: usize = counts
            .iter()
            .filter(|(s, _)| s.as_str() == "READY")
            .map(|(_, c)| *c)
            .sum();
        match FtsIndex::open(&home.tantivy_dir()) {
            Ok(fts) => {
                let fts_count = fts.num_segments().unwrap_or(0);
                let (status, message) = if ready_count > 0 && fts_count == 0 {
                    (
                        "warn",
                        format!(
                            "{ready_count} READY documents but 0 FTS segments — run `shiro reindex`"
                        ),
                    )
                } else {
                    (
                        "ok",
                        format!("{ready_count} READY documents, {fts_count} FTS segments"),
                    )
                };
                checks.push(DoctorCheck {
                    name: "fts_consistency".into(),
                    status: status.into(),
                    message,
                    details: None,
                });
            }
            Err(_) => {
                checks.push(DoctorCheck {
                    name: "fts_consistency".into(),
                    status: "fail".into(),
                    message: "cannot reopen FTS index for consistency check".into(),
                    details: None,
                });
            }
        }
    }

    // Check 7: vector index (optional)
    if input.verify_vector {
        let vector_dir = home.vector_dir();
        let vec_data = vector_dir.join("vectors.jsonl");
        if !vector_dir.as_std_path().is_dir() || !vec_data.as_std_path().exists() {
            checks.push(DoctorCheck {
                name: "vector_index".into(),
                status: "warn".into(),
                message: "no vector index found — vector search not yet configured".into(),
                details: None,
            });
        } else {
            match FlatIndex::open(384, vec_data) {
                Ok(idx) => {
                    let count = idx.count().unwrap_or(0);
                    checks.push(DoctorCheck {
                        name: "vector_index".into(),
                        status: "ok".into(),
                        message: format!("{count} vectors indexed, dims={}", idx.dimensions()),
                        details: None,
                    });
                }
                Err(e) => {
                    checks.push(DoctorCheck {
                        name: "vector_index".into(),
                        status: "fail".into(),
                        message: format!("cannot open vector index: {e}"),
                        details: None,
                    });
                }
            }
        }
    }

    let healthy = home_exists && db_ok && fts_ok;

    Ok(DoctorOutput { healthy, checks })
}
