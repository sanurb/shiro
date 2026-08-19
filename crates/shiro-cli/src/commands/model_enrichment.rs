use camino::Utf8Path;
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{
    Engine, ModelEnrichmentProposalInput, ModelEnrichmentResolutionAction,
    ModelEnrichmentResolutionInput,
};

use crate::envelope::{CmdOutput, NextAction};

pub fn run_propose(home: &ShiroHome, file: &Utf8Path) -> Result<CmdOutput, ShiroError> {
    let bytes = std::fs::read(file)?;
    let input: ModelEnrichmentProposalInput =
        serde_json::from_slice(&bytes).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid model enrichment proposal JSON: {error}"),
        })?;
    let engine = Engine::open(home.clone())?;
    let output = engine.propose_model_enrichment(&input)?;
    Ok(CmdOutput {
        result: serde_json::to_value(output).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize model enrichment proposal: {error}"),
        })?,
        next_actions: vec![NextAction::simple(
            "shiro enrich-model resolve <proposal_id> --action promote --actor <id> --approval <id>",
            "Explicitly promote or reject the proposal",
        )],
    })
}

pub fn run_resolve(
    home: &ShiroHome,
    proposal_id: &str,
    action: ModelEnrichmentResolutionAction,
    actor: &str,
    approval: &str,
) -> Result<CmdOutput, ShiroError> {
    let engine = Engine::open(home.clone())?;
    let output = engine.resolve_model_enrichment(&ModelEnrichmentResolutionInput {
        proposal_id: proposal_id.to_string(),
        action,
        resolved_actor_id: actor.to_string(),
        approval_id: approval.to_string(),
    })?;
    Ok(CmdOutput {
        result: serde_json::to_value(output).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize model enrichment resolution: {error}"),
        })?,
        next_actions: vec![NextAction::simple(
            "shiro taxonomy browse",
            "Inspect trusted taxonomy state",
        )],
    })
}
