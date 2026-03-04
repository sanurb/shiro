use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

// TODO: create .shiro/ directory structure (staging/, live/, config.toml). Acceptance: idempotent re-init.

#[derive(Debug, Serialize)]
pub(crate) struct InitData {
    data_dir: String,
    created: bool,
}

impl fmt::Display for InitData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.created {
            write!(f, "Initialized shiro data directory at {}", self.data_dir)
        } else {
            write!(
                f,
                "shiro data directory already exists at {}",
                self.data_dir
            )
        }
    }
}

pub(crate) fn run(json: bool) -> i32 {
    let data = InitData {
        data_dir: ".shiro".to_string(),
        created: true,
    };

    let next_actions = [NextAction {
        command: "shiro add <path>".to_string(),
        description: "Add a file to the shiro store".to_string(),
    }];

    envelope::print_success("shiro init", &data, &next_actions, json)
}
