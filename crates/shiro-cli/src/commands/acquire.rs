use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{AcquireUrlInput, AcquisitionParser};

use crate::envelope::{CmdOutput, NextAction};

pub fn run(
    home: &ShiroHome,
    url: &str,
    parser: AcquisitionParser,
    max_bytes: usize,
    timeout_ms: u64,
    max_redirects: usize,
    allow_http: bool,
) -> Result<CmdOutput, ShiroError> {
    let mut engine = crate::runtime::open_engine(home, crate::runtime::RuntimeProfile::Vector)?;
    let output = engine.acquire_url_incremental(
        &AcquireUrlInput {
            url: url.to_string(),
            parser,
            max_bytes,
            timeout_ms,
            max_redirects,
            allow_http,
        },
        32,
    )?;
    Ok(CmdOutput {
        result: serde_json::to_value(output).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize URL acquisition: {error}"),
        })?,
        next_actions: vec![
            NextAction::simple("shiro read <doc_id>", "Read the acquired document"),
            NextAction::simple("shiro search <query>", "Search the acquired evidence"),
        ],
    })
}
