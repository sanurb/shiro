//! `doctor` — consistency checks and diagnostics.

use serde::{Deserialize, Serialize};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::{artifact_digest, FtsIndex};
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

    // Check 3: active Tantivy generation and common-manifest digest.
    let active_fts_generation = store_opt
        .as_ref()
        .and_then(|store| store.active_generation("fts").ok())
        .map(|generation| generation.as_u64())
        .unwrap_or(0);
    let active_fts_path = home.tantivy_generation_dir(active_fts_generation);
    let fts_ok = match FtsIndex::open_generation(&active_fts_path, active_fts_generation) {
        Ok(fts) => {
            let count = fts.num_segments().unwrap_or(0);
            let valid = match store_opt
                .as_ref()
                .and_then(|store| store.active_corpus_manifest().ok().flatten())
                .filter(|manifest| !manifest.fts_digest.is_empty())
            {
                Some(manifest) => artifact_digest(&active_fts_path)
                    .map(|actual| actual == manifest.fts_digest)
                    .unwrap_or(false),
                None => true,
            };
            checks.push(DoctorCheck {
                name: "fts_index".into(),
                status: if valid { "ok" } else { "fail" }.into(),
                message: if valid {
                    format!(
                        "{count} segments indexed in generation {active_fts_generation}"
                    )
                } else {
                    format!(
                        "generation {active_fts_generation} does not match its corpus manifest digest"
                    )
                },
                details: None,
            });
            valid
        }
        Err(e) => {
            checks.push(DoctorCheck {
                name: "fts_index".into(),
                status: "fail".into(),
                message: format!("cannot open FTS generation {active_fts_generation}: {e}"),
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
                (
                    "ok",
                    format!("{ready_count} READY documents have processing fingerprints"),
                )
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
        match FtsIndex::open_generation(&active_fts_path, active_fts_generation) {
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

    // Check 8: active vector generation (optional).
    if input.verify_vector {
        let active_vector_generation = store_opt
            .as_ref()
            .and_then(|store| store.active_generation("vector").ok())
            .map(|generation| generation.as_u64())
            .unwrap_or(0);
        let vector_data_path = home.vector_data_path(active_vector_generation);
        let fingerprint_path = home
            .vector_generation_dir(active_vector_generation)
            .join("flat.fingerprint.json");
        let vector_published = store_opt
            .as_ref()
            .and_then(|store| store.active_corpus_manifest().ok().flatten())
            .map(|manifest| manifest.vector_generation.is_some())
            .unwrap_or(true);
        if !vector_published {
            checks.push(DoctorCheck {
                name: "vector_index".into(),
                status: "warn".into(),
                message: "active corpus manifest has no vector generation — run `shiro reindex`"
                    .into(),
                details: None,
            });
        } else {
            match std::fs::read_to_string(vector_data_path.as_std_path()) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    checks.push(DoctorCheck {
                        name: "vector_index".into(),
                        status: "warn".into(),
                        message: "no vector index found — vector search not yet configured".into(),
                        details: None,
                    });
                }
                Err(error) => {
                    checks.push(DoctorCheck {
                        name: "vector_index".into(),
                        status: "fail".into(),
                        message: format!("cannot read vector index: {error}"),
                        details: Some(serde_json::json!({
                            "data_path": vector_data_path.as_str(),
                        })),
                    });
                }
                Ok(content) => {
                    let vector_count = content
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .count();
                    let fingerprint_read = std::fs::read_to_string(fingerprint_path.as_std_path());
                    let (status, message, fingerprint_hash) = match fingerprint_read {
                    Ok(json) => {
                        match serde_json::from_str::<shiro_core::EmbeddingFingerprint>(&json) {
                            Ok(fingerprint) => (
                                "ok",
                                format!(
                                    "{vector_count} vectors indexed with embedding fingerprint"
                                ),
                                Some(fingerprint.fingerprint_hash),
                            ),
                            Err(error) => (
                                "fail",
                                format!(
                                    "{vector_count} vectors indexed with malformed embedding fingerprint: {error}"
                                ),
                                None,
                            ),
                        }
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound && vector_count == 0 =>
                    {
                        ("ok", "empty vector index".to_string(), None)
                    }
                    Err(error) => (
                        "fail",
                        format!(
                            "{vector_count} vectors indexed without a readable embedding fingerprint: {error}"
                        ),
                        None,
                    ),
                };
                    let digest_valid = store_opt
                        .as_ref()
                        .and_then(|store| store.active_corpus_manifest().ok().flatten())
                        .and_then(|manifest| manifest.vector_digest)
                        .map(|expected| {
                            artifact_digest(&home.vector_generation_dir(active_vector_generation))
                                .map(|actual| actual == expected)
                                .unwrap_or(false)
                        })
                        .unwrap_or(true);
                    checks.push(DoctorCheck {
                    name: "vector_index".into(),
                    status: if status == "ok" && !digest_valid {
                        "fail".into()
                    } else {
                        status.into()
                    },
                    message: if digest_valid {
                        message
                    } else {
                        format!(
                            "vector generation {active_vector_generation} does not match its corpus manifest digest"
                        )
                    },
                    details: Some(serde_json::json!({
                        "data_path": vector_data_path.as_str(),
                        "fingerprint_path": fingerprint_path.as_str(),
                        "fingerprint_hash": fingerprint_hash,
                    })),
                });
                }
            }
        }
    }

    let healthy = home_exists && db_ok && fts_ok;

    Ok(DoctorOutput { healthy, checks })
}
