use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{ReprocessInput, ReprocessLimits, ReprocessTarget};

use crate::envelope::{CmdOutput, NextAction};

pub struct ReprocessOptions<'a> {
    pub document_ids: &'a [String],
    pub parser_name: &'a str,
    pub target: ReprocessTarget,
    pub execute: bool,
    pub include_vector: bool,
    pub resume_manifest_id: Option<&'a str>,
    pub max_documents: usize,
    pub max_source_bytes: usize,
    pub max_model_calls: usize,
    pub embedding_batch_size: usize,
}

pub fn run(home: &ShiroHome, options: ReprocessOptions<'_>) -> Result<CmdOutput, ShiroError> {
    let profile = if options.include_vector {
        crate::runtime::RuntimeProfile::Vector
    } else {
        crate::runtime::RuntimeProfile::Base
    };
    let mut engine = crate::runtime::open_engine(home, profile)?;
    let parser = crate::commands::select_parser(options.parser_name, None)?;
    let output = engine.reprocess(
        parser.as_ref(),
        &ReprocessInput {
            document_ids: options.document_ids.to_vec(),
            target: options.target,
            execute: options.execute,
            include_vector: options.include_vector,
            resume_manifest_id: options.resume_manifest_id.map(str::to_string),
            limits: ReprocessLimits {
                max_documents: options.max_documents,
                max_source_bytes: options.max_source_bytes,
                max_model_calls: options.max_model_calls,
                embedding_batch_size: options.embedding_batch_size,
            },
        },
    )?;
    Ok(CmdOutput {
        result: serde_json::to_value(output).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize reprocess output: {error}"),
        })?,
        next_actions: vec![
            NextAction::simple(
                "shiro reprocess --execute",
                "Execute this plan with the same limits and rollback manifest",
            ),
            NextAction::simple("shiro doctor", "Verify the active corpus after execution"),
        ],
    })
}
