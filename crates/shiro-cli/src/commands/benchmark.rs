//! `shiro benchmark` — evaluate a versioned judged corpus and rebuild integrity.

use camino::Utf8Path;
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::BenchmarkManifest;

use crate::envelope::{CmdOutput, NextAction};
use crate::runtime::{open_engine, open_engine_for_reindex, RuntimeProfile};

pub fn run(
    home: &ShiroHome,
    manifest_path: &Utf8Path,
    warmup_runs: usize,
    measured_runs: usize,
) -> Result<CmdOutput, ShiroError> {
    let bytes =
        std::fs::read(manifest_path.as_std_path()).map_err(|error| ShiroError::InvalidInput {
            message: format!("failed to read benchmark manifest {manifest_path}: {error}"),
        })?;
    let manifest: BenchmarkManifest =
        serde_json::from_slice(&bytes).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid benchmark manifest {manifest_path}: {error}"),
        })?;

    let engine = open_engine(home, RuntimeProfile::Full)?;
    let before = engine.benchmark(&manifest, warmup_runs, measured_runs)?;
    drop(engine);

    // Rebuild is mandatory evidence under ADR-027. The reindex composition uses
    // the configured embedder when present and one common manifest activation.
    let reindex_engine = open_engine_for_reindex(home)?;
    reindex_engine.reindex_all()?;
    drop(reindex_engine);

    let rebuilt_engine = open_engine(home, RuntimeProfile::Full)?;
    let rebuilt = rebuilt_engine.benchmark(&manifest, 0, 1)?;
    let output = before.with_rebuild_integrity(&rebuilt);
    let result = serde_json::to_value(output).map_err(|error| ShiroError::StoreCorrupt {
        message: format!("failed to serialize benchmark result: {error}"),
    })?;

    Ok(CmdOutput {
        result,
        next_actions: vec![NextAction::simple(
            format!("shiro benchmark {manifest_path}"),
            "Repeat on the declared hardware profile",
        )],
    })
}
