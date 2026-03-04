use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[derive(Serialize)]
struct CommandInfo {
    name: &'static str,
    description: &'static str,
    usage: &'static str,
}

#[derive(Serialize)]
pub(crate) struct RootData {
    description: &'static str,
    version: &'static str,
    commands: Vec<CommandInfo>,
}

impl fmt::Display for RootData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "shiro v{}", self.version)?;
        writeln!(f, "{}", self.description)?;
        writeln!(f)?;
        writeln!(f, "commands:")?;
        for cmd in &self.commands {
            writeln!(f, "  {:10} {}", cmd.name, cmd.description)?;
        }
        Ok(())
    }
}

pub(crate) fn run(json: bool) -> i32 {
    let data = RootData {
        description: "local-first document knowledge engine",
        version: env!("CARGO_PKG_VERSION"),
        commands: vec![
            CommandInfo {
                name: "init",
                description: "Initialize a shiro data directory",
                usage: "shiro init",
            },
            CommandInfo {
                name: "add",
                description: "Add a file to the staging area",
                usage: "shiro add <path>",
            },
            CommandInfo {
                name: "ingest",
                description: "Ingest staged documents",
                usage: "shiro ingest",
            },
            CommandInfo {
                name: "search",
                description: "Search indexed documents",
                usage: "shiro search <query>",
            },
            CommandInfo {
                name: "doctor",
                description: "Run diagnostic checks",
                usage: "shiro doctor",
            },
            CommandInfo {
                name: "config",
                description: "Show or manage configuration",
                usage: "shiro config",
            },
            CommandInfo {
                name: "status",
                description: "Show system status",
                usage: "shiro status",
            },
        ],
    };

    let next_actions = [NextAction {
        command: "shiro init".into(),
        description: "Get started by initializing a data directory".into(),
    }];

    envelope::print_success("shiro", &data, &next_actions, json)
}
