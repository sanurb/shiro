//! `shiro` — local-first document knowledge engine CLI.
//!
//! JSON-only output. Logs to stderr.
//! See `docs/CLI.md` for the full command contract.

use clap::{Parser, Subcommand, ValueEnum};
use shiro_core::{ShiroError, ShiroHome};

mod commands;
mod envelope;

use envelope::{print_error, print_success, CmdOutput, NextAction};

// ---------------------------------------------------------------------------
// CLI definition (clap v4 derive)
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "shiro",
    version,
    about = "Local-first document knowledge engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override the data directory (default: ~/.shiro or $SHIRO_HOME).
    #[arg(long, global = true, env = "SHIRO_HOME")]
    home: Option<String>,

    /// Log level for stderr output.
    #[arg(long, global = true, default_value = "warn")]
    log_level: LogLevel,
}

#[derive(Clone, Copy, ValueEnum)]
enum LogLevel {
    Silent,
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    fn as_filter(self) -> &'static str {
        match self {
            Self::Silent => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a shiro data directory.
    Init,

    /// Add a file to the library (parse, index, activate).
    Add {
        /// Path or URL of the file to add.
        path: String,

        /// Parser backend.
        #[arg(long, value_enum, default_value = "baseline")]
        parser: ParserChoice,

        /// Enable LLM-based enrichment (tags, concepts).
        #[arg(long)]
        enrich: bool,

        /// Comma-separated tags to attach.
        #[arg(long)]
        tags: Option<String>,

        /// Comma-separated concept IDs to attach.
        #[arg(long)]
        concepts: Option<String>,

        /// Skip vector indexing; FTS only.
        #[arg(long)]
        fts_only: bool,

        /// Stream NDJSON progress to stdout.
        #[arg(long)]
        follow: bool,
    },

    /// Batch-ingest documents from directories.
    Ingest {
        /// Directories to scan.
        dirs: Vec<std::path::PathBuf>,

        /// File glob pattern (default: *.{txt,md}).
        #[arg(long)]
        glob: Option<String>,

        /// Parser backend.
        #[arg(long, value_enum, default_value = "baseline")]
        parser: ParserChoice,

        /// Enable LLM-based enrichment.
        #[arg(long)]
        enrich: bool,

        /// Comma-separated tags to attach.
        #[arg(long)]
        tags: Option<String>,

        /// Comma-separated concept IDs to attach.
        #[arg(long)]
        concepts: Option<String>,

        /// Maximum number of files to process.
        #[arg(long)]
        max_files: Option<usize>,

        /// Skip vector indexing; FTS only.
        #[arg(long)]
        fts_only: bool,

        /// Stream NDJSON progress to stdout.
        #[arg(long)]
        follow: bool,
    },

    /// Search indexed documents.
    Search {
        /// Search query.
        query: String,

        /// Search mode.
        #[arg(long, value_enum, default_value = "hybrid")]
        mode: SearchModeArg,

        /// Maximum number of results.
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Expand results with surrounding context.
        #[arg(long)]
        expand: bool,

        /// Max blocks when expanding.
        #[arg(long, default_value = "12")]
        max_blocks: usize,

        /// Max chars when expanding.
        #[arg(long, default_value = "8000")]
        max_chars: usize,

        /// Filter by tag.
        #[arg(long)]
        tag: Option<String>,

        /// Filter by concept ID.
        #[arg(long)]
        concept: Option<String>,

        /// Filter by document ID.
        #[arg(long)]
        doc: Option<String>,
    },

    /// Read document content.
    Read {
        /// Document ID or title.
        id: String,

        /// View mode: outline, text, or blocks.
        #[arg(long, value_enum, default_value = "text")]
        view: ReadView,
    },

    /// Explain why a search result matched.
    Explain {
        /// Result ID from a search.
        result_id: String,
    },

    /// List documents in the library.
    List {
        /// Maximum number of documents to show.
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Filter by tag.
        #[arg(long)]
        tag: Option<String>,

        /// Filter by concept ID.
        #[arg(long)]
        concept: Option<String>,
    },

    /// Remove a document from the library.
    Remove {
        /// Document ID or title.
        id: String,

        /// Purge from derived indices immediately.
        #[arg(long)]
        purge: bool,
    },

    /// Run diagnostic checks on the library.
    Doctor {
        /// Verify vector index integrity.
        #[arg(long)]
        verify_vector: bool,

        /// Attempt repair of detected issues.
        #[arg(long)]
        repair: bool,
    },

    /// Show or manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ParserChoice {
    Baseline,
    Premium,
}

#[derive(Clone, Copy, ValueEnum)]
enum SearchModeArg {
    Hybrid,
    Bm25,
    Vector,
}

#[derive(Clone, Copy, ValueEnum)]
enum ReadView {
    Outline,
    Text,
    Blocks,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show all configuration.
    Show,
    /// Get a configuration value.
    Get {
        /// Configuration key.
        key: String,
    },
    /// Set a configuration value.
    Set {
        /// Configuration key.
        key: String,
        /// New value.
        value: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    // Initialize tracing to stderr.
    let filter = cli.log_level.as_filter();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_writer(std::io::stderr)
        .init();

    let cmd_name = command_name(&cli);

    let code = match dispatch(&cli) {
        Ok(output) => print_success(cmd_name, &output),
        Err(err) => {
            let fix = suggest_fix(&err);
            let next = recovery_actions(&err);
            print_error(cmd_name, &err, fix, &next)
        }
    };

    std::process::exit(code);
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn dispatch(cli: &Cli) -> Result<CmdOutput, ShiroError> {
    match &cli.command {
        None => commands::root::run(),

        Some(Commands::Init) => {
            let home = resolve_home(cli)?;
            commands::init::run(&home)
        }

        Some(Commands::Add { path, .. }) => {
            let home = resolve_home(cli)?;
            commands::add::run(&home, path)
        }

        Some(Commands::Ingest {
            dirs,
            glob,
            max_files,
            ..
        }) => {
            let home = resolve_home(cli)?;
            commands::ingest::run(&home, dirs, glob.as_deref(), *max_files)
        }

        Some(Commands::Search {
            query, mode, limit, ..
        }) => {
            let home = resolve_home(cli)?;
            let m = match mode {
                SearchModeArg::Hybrid => commands::search::SearchMode::Hybrid,
                SearchModeArg::Bm25 => commands::search::SearchMode::Bm25,
                SearchModeArg::Vector => commands::search::SearchMode::Vector,
            };
            commands::search::run(&home, query, m, *limit)
        }

        Some(Commands::Read { id, view }) => {
            let home = resolve_home(cli)?;
            let m = match view {
                ReadView::Text => commands::read::ReadMode::Text,
                ReadView::Blocks => commands::read::ReadMode::Blocks,
                ReadView::Outline => commands::read::ReadMode::Outline,
            };
            commands::read::run(&home, id, m)
        }

        Some(Commands::Explain { result_id }) => {
            let home = resolve_home(cli)?;
            commands::explain::run(&home, result_id)
        }

        Some(Commands::List { limit, .. }) => {
            let home = resolve_home(cli)?;
            commands::list::run(&home, *limit)
        }

        Some(Commands::Remove { id, purge }) => {
            let home = resolve_home(cli)?;
            commands::remove::run(&home, id, *purge)
        }

        Some(Commands::Doctor { .. }) => {
            let home = resolve_home(cli)?;
            commands::doctor::run(&home)
        }

        Some(Commands::Config { action }) => {
            let home = resolve_home(cli)?;
            match action {
                ConfigAction::Show => commands::config::run_show(&home),
                ConfigAction::Get { key } => commands::config::run_get(&home, key),
                ConfigAction::Set { key, value } => commands::config::run_set(&home, key, value),
            }
        }
    }
}

fn resolve_home(cli: &Cli) -> Result<ShiroHome, ShiroError> {
    ShiroHome::resolve(cli.home.as_deref()).map_err(|e| ShiroError::Config { message: e })
}

fn command_name(cli: &Cli) -> &'static str {
    match &cli.command {
        None => "shiro",
        Some(Commands::Init) => "shiro init",
        Some(Commands::Add { .. }) => "shiro add",
        Some(Commands::Ingest { .. }) => "shiro ingest",
        Some(Commands::Search { .. }) => "shiro search",
        Some(Commands::Read { .. }) => "shiro read",
        Some(Commands::Explain { .. }) => "shiro explain",
        Some(Commands::List { .. }) => "shiro list",
        Some(Commands::Remove { .. }) => "shiro remove",
        Some(Commands::Doctor { .. }) => "shiro doctor",
        Some(Commands::Config { .. }) => "shiro config",
    }
}

fn suggest_fix(err: &ShiroError) -> Option<&'static str> {
    match err {
        ShiroError::LockBusy { .. } => {
            Some("Another shiro process may be running. Wait or run `shiro doctor`.")
        }
        ShiroError::StoreCorrupt { .. } => Some("Run `shiro doctor --repair` to attempt recovery."),
        ShiroError::ParsePdf { .. } => {
            Some("Ensure the file is a valid PDF. Try `--parser premium` if configured.")
        }
        ShiroError::Config { .. } => Some("Check SHIRO_HOME or run `shiro init`."),
        _ => None,
    }
}

fn recovery_actions(err: &ShiroError) -> Vec<NextAction> {
    match err {
        ShiroError::StoreCorrupt { .. } => {
            vec![NextAction::simple(
                "shiro doctor --repair",
                "Attempt repair",
            )]
        }
        ShiroError::LockBusy { .. } => {
            vec![NextAction::simple("shiro doctor", "Check for stale locks")]
        }
        _ => vec![NextAction::simple("shiro doctor", "Run diagnostics")],
    }
}
