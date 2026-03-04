use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[derive(Serialize)]
pub(crate) struct StatusData {
    version: &'static str,
    status: &'static str,
}

impl fmt::Display for StatusData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shiro v{}: {}", self.version, self.status)
    }
}

pub(crate) fn run(json: bool) -> i32 {
    let data = StatusData {
        version: env!("CARGO_PKG_VERSION"),
        status: "ok",
    };

    let next_actions = [NextAction {
        command: "shiro".into(),
        description: "Show all commands".into(),
    }];

    envelope::print_success("shiro status", &data, &next_actions, json)
}
