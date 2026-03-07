//! `shiro completions` — generate shell completions.

use shiro_core::ShiroError;
use std::io;

/// Shell variants for completion generation.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
}

/// Generate shell completions to stdout.
///
/// This is the ONE command that writes raw text (not JSON) to stdout,
/// because shell completions are consumed by the shell directly.
pub fn run(shell: CompletionShell, cmd: &mut clap::Command) -> Result<(), ShiroError> {
    let shell = match shell {
        CompletionShell::Bash => clap_complete::Shell::Bash,
        CompletionShell::Zsh => clap_complete::Shell::Zsh,
        CompletionShell::Fish => clap_complete::Shell::Fish,
        CompletionShell::PowerShell => clap_complete::Shell::PowerShell,
    };
    clap_complete::generate(shell, cmd, "shiro", &mut io::stdout());
    Ok(())
}
