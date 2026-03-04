// TODO: read/write config.toml, support get/set subcommands. Acceptance: round-trips config values.

use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[derive(Serialize)]
pub(crate) struct ConfigData {
    data_dir: String,
    config_path: String,
}

impl fmt::Display for ConfigData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "data_dir:    {}", self.data_dir)?;
        write!(f, "config_path: {}", self.config_path)
    }
}

pub(crate) fn run(json: bool) -> i32 {
    let data = ConfigData {
        data_dir: "~/.local/share/shiro".into(),
        config_path: "~/.config/shiro/config.toml".into(),
    };

    let next_actions = [
        NextAction {
            command: "shiro init".into(),
            description: "Initialize shiro data directory".into(),
        },
        NextAction {
            command: "shiro doctor".into(),
            description: "Check configuration health".into(),
        },
    ];

    envelope::print_success("shiro config", &data, &next_actions, json)
}
