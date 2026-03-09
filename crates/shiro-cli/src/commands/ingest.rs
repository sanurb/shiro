//! `shiro ingest` — batch-add documents from directories.

use crate::commands::select_parser;
use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{Engine, IngestEvent, IngestInput};

pub fn run(
    home: &ShiroHome,
    dirs: &[std::path::PathBuf],
    max_files: Option<usize>,
    follow: bool,
    parser_name: &str,
) -> Result<CmdOutput, ShiroError> {
    let engine = Engine::open(home.clone())?;
    let parser = select_parser(parser_name, None)?;

    let input = IngestInput {
        dirs: dirs
            .iter()
            .map(|d| d.to_string_lossy().to_string())
            .collect(),
        max_files,
    };

    let cb: &dyn Fn(&IngestEvent) = &emit_event;
    let on_event: Option<&dyn Fn(&IngestEvent)> = if follow { Some(cb) } else { None };

    let output = engine.ingest(parser.as_ref(), &input, on_event)?;

    let result = serde_json::json!({
        "added": output.added,
        "ready": output.ready,
        "failed": output.failed,
        "failures": output.failures,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro list", "List documents"),
            NextAction::simple("shiro search <query>", "Search the library"),
        ],
    })
}

fn emit_event(event: &IngestEvent) {
    if let Ok(line) = serde_json::to_string(event) {
        eprintln!("{line}");
    }
}
