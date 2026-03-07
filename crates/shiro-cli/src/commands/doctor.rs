//! `shiro doctor` — consistency checks and diagnostics.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};

pub fn run(home: &ShiroHome, verify_vector: bool) -> Result<CmdOutput, ShiroError> {
    let input = shiro_sdk::ops::doctor::DoctorInput { verify_vector };
    let output = shiro_sdk::ops::doctor::execute(home, &input)?;

    let result = serde_json::to_value(&output).map_err(|e| ShiroError::InvalidInput {
        message: format!("serialization failed: {e}"),
    })?;

    let mut next_actions = Vec::new();
    if !output.healthy {
        next_actions.push(NextAction::simple(
            "shiro init",
            "Re-initialize the library",
        ));
    }
    next_actions.push(NextAction::simple("shiro list", "List documents"));

    Ok(CmdOutput {
        result,
        next_actions,
    })
}
