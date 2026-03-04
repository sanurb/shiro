// TODO: create RunManifest, parse staged docs, index segments, promote to live. Acceptance: segments queryable after ingest.

use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[derive(Serialize)]
pub(crate) struct IngestData {
    run_id: String,
    documents_processed: usize,
    segments_created: usize,
}

impl fmt::Display for IngestData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "run {}: {} docs processed, {} segments created",
            self.run_id, self.documents_processed, self.segments_created
        )
    }
}

pub(crate) fn run(json: bool) -> i32 {
    let data = IngestData {
        run_id: "run-stub".into(),
        documents_processed: 0,
        segments_created: 0,
    };

    let next_actions = [NextAction {
        command: "shiro search <query>".into(),
        description: "Search ingested segments".into(),
    }];

    envelope::print_success("shiro ingest", &data, &next_actions, json)
}
