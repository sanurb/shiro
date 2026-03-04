// TODO: copy/link file to staging, compute DocId from content. Acceptance: file exists in staging after add.

use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[derive(Serialize)]
pub(crate) struct AddData {
    path: String,
    doc_id: String,
    staged: bool,
}

impl fmt::Display for AddData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "added {} (doc_id: {}, staged: {})",
            self.path, self.doc_id, self.staged
        )
    }
}

pub(crate) fn run(path: &str, json: bool) -> i32 {
    let data = AddData {
        path: path.to_owned(),
        doc_id: "<not-yet-computed>".into(),
        staged: false,
    };

    let next_actions = [NextAction {
        command: "shiro ingest".into(),
        description: "Ingest staged documents".into(),
    }];

    envelope::print_success("shiro add", &data, &next_actions, json)
}
