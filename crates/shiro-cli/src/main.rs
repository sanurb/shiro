use clap::{Parser, Subcommand};
use shiro_core::ShiroError;
use tracing_subscriber::EnvFilter;

mod commands;
mod envelope;

#[derive(Parser)]
#[command(
    name = "shiro",
    version,
    about = "Local-first document knowledge engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output format: json (default) or text
    #[arg(long, global = true, default_value = "json")]
    format: Format,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Json,
    Text,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a shiro data directory
    Init,
    /// Add a file to the staging area
    Add {
        /// Path to the file to add
        path: String,
    },
    /// Ingest staged documents (parse, index, promote)
    Ingest,
    /// Search indexed documents
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Run diagnostic checks
    Doctor,
    /// Show or manage configuration
    Config,
    /// Show system status
    Status,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let json = matches!(cli.format, Format::Json);

    // TODO: generate a real ULID/UUID run_id per invocation.
    // Acceptance: run_id appears in tracing spans and manifest entries.
    let run_id = "run-0000";

    let code = {
        let _span = tracing::info_span!("shiro", %run_id).entered();
        match dispatch(&cli, json) {
            Ok(code) => code,
            Err(err) => envelope::print_error(command_name(&cli), &err, None, &[], json),
        }
    };

    std::process::exit(code);
}

fn command_name(cli: &Cli) -> &'static str {
    match &cli.command {
        Some(Commands::Init) => "shiro init",
        Some(Commands::Add { .. }) => "shiro add",
        Some(Commands::Ingest) => "shiro ingest",
        Some(Commands::Search { .. }) => "shiro search",
        Some(Commands::Doctor) => "shiro doctor",
        Some(Commands::Config) => "shiro config",
        Some(Commands::Status) => "shiro status",
        None => "shiro",
    }
}

fn dispatch(cli: &Cli, json: bool) -> Result<i32, ShiroError> {
    Ok(match &cli.command {
        Some(Commands::Init) => commands::init::run(json),
        Some(Commands::Add { path }) => commands::add::run(path, json),
        Some(Commands::Ingest) => commands::ingest::run(json),
        Some(Commands::Search { query, limit }) => commands::search::run(query, *limit, json),
        Some(Commands::Doctor) => commands::doctor::run(json),
        Some(Commands::Config) => commands::config::run(json),
        Some(Commands::Status) => commands::status::run(json),
        None => commands::root::run(json),
    })
}
