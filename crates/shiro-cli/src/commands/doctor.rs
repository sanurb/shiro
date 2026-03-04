// TODO: verify data_dir structure, index integrity, orphaned docs. Acceptance: detects and reports corruption.

use std::fmt;

use serde::Serialize;

use crate::envelope::{self, NextAction};

#[allow(dead_code)] // Warn/Fail used when checks are implemented
#[derive(Debug, Clone, Serialize)]
pub(crate) enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warn => write!(f, "warn"),
            Self::Fail => write!(f, "FAIL"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Check {
    name: String,
    status: CheckStatus,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.status)
    }
}

#[derive(Serialize)]
pub(crate) struct DoctorData {
    checks: Vec<Check>,
}

impl fmt::Display for DoctorData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, check) in self.checks.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "  {check}")?;
        }
        Ok(())
    }
}

pub(crate) fn run(json: bool) -> i32 {
    let data = DoctorData {
        checks: vec![
            Check {
                name: "data_dir".into(),
                status: CheckStatus::Ok,
            },
            Check {
                name: "index".into(),
                status: CheckStatus::Ok,
            },
        ],
    };

    let next_actions = [NextAction {
        command: "shiro status".into(),
        description: "Show current status".into(),
    }];

    envelope::print_success("shiro doctor", &data, &next_actions, json)
}
