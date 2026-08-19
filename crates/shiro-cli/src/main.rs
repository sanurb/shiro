//! `shiro` — local-first document knowledge engine CLI.
//!
//! JSON-only output. Logs to stderr.
//! See `docs/CLI.md` for the full command contract.

use clap::{Parser, Subcommand, ValueEnum};
use shiro_core::{ShiroError, ShiroHome};

mod commands;
mod envelope;
mod runtime;
mod schema;

use commands::completions::CompletionShell;
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

        /// Parser to use (auto, plaintext, markdown, pdf, docling).
        #[arg(long, default_value = "auto")]
        parser: String,
    },

    /// Safely acquire and ingest a bounded remote source.
    AcquireUrl {
        /// HTTPS URL for a PDF, Markdown, or UTF-8 text source.
        url: String,

        #[arg(long, value_enum, default_value = "auto")]
        parser: AcquisitionParserArg,

        #[arg(long, default_value_t = 52_428_800)]
        max_bytes: usize,

        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,

        #[arg(long, default_value_t = 5)]
        max_redirects: usize,

        /// Permit unencrypted HTTP; HTTPS is required by default.
        #[arg(long)]
        allow_http: bool,
    },

    /// Batch-ingest documents from directories.
    Ingest {
        /// Directories to scan.
        dirs: Vec<std::path::PathBuf>,

        /// Maximum number of files to process.
        #[arg(long)]
        max_files: Option<usize>,

        /// Stream NDJSON progress to stdout.
        #[arg(long)]
        follow: bool,

        /// Parser to use (auto, plaintext, markdown, pdf, docling).
        #[arg(long, default_value = "auto")]
        parser: String,
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

        /// Apply reranker to fused results.
        #[arg(long)]
        rerank: bool,

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

    /// Run multiple queries and deduplicate stable evidence handles.
    SearchPack {
        /// Query strings to execute as one pack.
        #[arg(required = true, num_args = 1..)]
        queries: Vec<String>,

        /// Search mode: hybrid, bm25, or vector.
        #[arg(long, value_enum, default_value = "hybrid")]
        mode: SearchModeArg,

        /// Maximum candidates retained from each query.
        #[arg(long, default_value_t = 10)]
        per_query_limit: usize,

        /// Maximum deduplicated evidence handles.
        #[arg(long, default_value_t = 20)]
        limit: usize,

        /// Include snippets and context blocks.
        #[arg(long)]
        include_content: bool,

        /// Apply the configured reranker to each query.
        #[arg(long)]
        rerank: bool,

        /// Restrict to documents with any matching tag.
        #[arg(long = "tag")]
        tags: Vec<String>,

        /// Restrict to documents assigned to any matching concept ID.
        #[arg(long = "concept")]
        concept_ids: Vec<String>,

        /// Restrict to any matching document ID.
        #[arg(long = "doc")]
        document_ids: Vec<String>,
    },

    /// Read document content.
    Read {
        /// Document ID, evidence handle, or title.
        id: String,

        /// View mode: outline, text, or blocks.
        #[arg(long, value_enum, default_value = "text")]
        view: ReadView,

        /// Read all blocks attributed to this one-based source page.
        #[arg(long)]
        page: Option<u32>,
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
    },

    /// Show or manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Describe shiro's capabilities as structured JSON.
    Capabilities,

    /// Start the MCP JSON-RPC server (reads from stdin, writes to stdout).
    Mcp {
        /// Permit write operations carrying explicit actor and approval IDs.
        #[arg(long)]
        allow_writes: bool,
    },

    /// Manage SKOS-style taxonomy concepts.
    Taxonomy {
        #[command(subcommand)]
        action: TaxonomyAction,
    },

    /// Rebuild FTS index from stored segments.
    Reindex,

    /// Plan or execute bounded scoped reprocessing.
    Reprocess {
        /// Optional document IDs; empty selects every READY document.
        #[arg(long = "doc")]
        document_ids: Vec<String>,

        /// Parser to apply to all selected source artifacts.
        #[arg(long)]
        parser: String,

        /// Reprocessing target.
        #[arg(long, value_enum, default_value = "all")]
        target: ReprocessTargetArg,

        /// Execute instead of returning a dry-run plan.
        #[arg(long)]
        execute: bool,

        /// Publish vectors with the configured embedder.
        #[arg(long)]
        include_vector: bool,

        /// Require this active verified manifest as the rollback point.
        #[arg(long)]
        resume_manifest_id: Option<String>,

        #[arg(long, default_value_t = 100)]
        max_documents: usize,

        #[arg(long, default_value_t = 536_870_912)]
        max_source_bytes: usize,

        #[arg(long, default_value_t = 100_000)]
        max_model_calls: usize,

        #[arg(long, default_value_t = 32)]
        embedding_batch_size: usize,
    },

    /// Run a versioned judged retrieval benchmark and rebuild-integrity check.
    Benchmark {
        /// Path to the benchmark manifest JSON.
        manifest: camino::Utf8PathBuf,

        /// Warmup executions per query and pipeline.
        #[arg(long, default_value = "1")]
        warmup_runs: usize,

        /// Measured executions per query and pipeline.
        #[arg(long, default_value = "3")]
        measured_runs: usize,
    },

    /// Generate shell completions.
    Completions {
        /// Target shell.
        shell: CompletionShell,
    },

    /// Run heuristic enrichment on a document.
    Enrich {
        /// Document ID or title.
        id: String,
    },

    /// Manage attributed, reversible model-enrichment proposals.
    EnrichModel {
        #[command(subcommand)]
        action: ModelEnrichmentAction,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum AcquisitionParserArg {
    Auto,
    Plaintext,
    Markdown,
    Pdf,
}

#[derive(Clone, Copy, ValueEnum)]
enum SearchModeArg {
    Hybrid,
    Bm25,
    Vector,
}

#[derive(Clone, Copy, ValueEnum)]
enum ReprocessTargetArg {
    Parse,
    Derived,
    All,
}

#[derive(Clone, Copy, ValueEnum)]
enum ReadView {
    Outline,
    Text,
    Blocks,
}

#[derive(Clone, Copy, ValueEnum)]
enum TaxonomyRelationKindArg {
    Broader,
    Narrower,
    Related,
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

#[derive(Subcommand)]
enum ModelEnrichmentAction {
    /// Store model output as an isolated PROPOSED record.
    Propose {
        /// JSON file matching ModelEnrichmentProposalInput.
        #[arg(long)]
        file: camino::Utf8PathBuf,
    },
    /// Explicitly promote or reject a proposal.
    Resolve {
        proposal_id: String,
        #[arg(long, value_enum)]
        action: ModelEnrichmentResolutionArg,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        approval: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ModelEnrichmentResolutionArg {
    Promote,
    Reject,
}

#[derive(Subcommand)]
enum TaxonomyAction {
    /// Add a concept to the taxonomy.
    Add {
        /// Scheme URI.
        #[arg(long)]
        scheme: String,

        /// Preferred label.
        #[arg(long)]
        label: String,

        /// Comma-separated alternative labels.
        #[arg(long)]
        alt_labels: Option<String>,

        /// Prose definition.
        #[arg(long)]
        definition: Option<String>,
    },

    /// List concepts.
    List {
        /// Maximum number of concepts.
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Search concept labels, synonyms, definitions, and schemes.
    Search {
        query: String,

        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Browse a bounded concept graph or list all concept roots.
    Browse {
        #[arg(long)]
        root: Option<String>,

        #[arg(long, default_value_t = 2)]
        max_depth: usize,

        #[arg(long, default_value_t = 100)]
        max_nodes: usize,
    },

    /// Show relations for a concept.
    Relations {
        /// Concept ID.
        concept_id: String,
    },

    /// Author a SKOS relation and rebuild hierarchical closure.
    Relate {
        /// Source concept ID.
        from_concept_id: String,

        /// Target concept ID.
        to_concept_id: String,

        /// Directed SKOS relation kind.
        #[arg(long, value_enum)]
        kind: TaxonomyRelationKindArg,
    },

    /// Assign a concept to a document.
    Assign {
        /// Document ID or title.
        doc_id: String,

        /// Concept ID.
        concept_id: String,

        /// Confidence score.
        #[arg(long, default_value_t = 1.0)]
        confidence: f32,

        /// Assignment source.
        #[arg(long, default_value = "manual")]
        source: String,
    },

    /// Import concepts from a SKOS JSON file.
    Import {
        /// Path to JSON file.
        file: std::path::PathBuf,
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

    // Completions bypass the JSON envelope — raw shell script to stdout.
    if let Some(Commands::Completions { shell }) = &cli.command {
        let mut cmd = <Cli as clap::CommandFactory>::command();
        if let Err(e) = commands::completions::run(*shell, &mut cmd) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let cmd_name = command_name(&cli);

    let start = std::time::Instant::now();

    let code = match dispatch(&cli) {
        Ok(output) => print_success(cmd_name, &output),
        Err(err) => {
            let fix = suggest_fix(&err);
            let next = recovery_actions(&err);
            print_error(cmd_name, &err, fix, &next)
        }
    };

    tracing::info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        command = cmd_name,
        exit_code = code,
        "completed"
    );

    std::process::exit(code);
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn dispatch(cli: &Cli) -> Result<CmdOutput, ShiroError> {
    let _span = tracing::info_span!("dispatch", command = command_name(cli)).entered();
    match &cli.command {
        None => commands::root::run(),

        Some(Commands::Init) => {
            let home = resolve_home(cli)?;
            commands::init::run(&home)
        }

        Some(Commands::Add { path, parser }) => {
            let home = resolve_home(cli)?;
            commands::add::run(&home, path, parser)
        }

        Some(Commands::AcquireUrl {
            url,
            parser,
            max_bytes,
            timeout_ms,
            max_redirects,
            allow_http,
        }) => {
            let home = resolve_home(cli)?;
            commands::acquire::run(
                &home,
                url,
                match parser {
                    AcquisitionParserArg::Auto => shiro_sdk::AcquisitionParser::Auto,
                    AcquisitionParserArg::Plaintext => shiro_sdk::AcquisitionParser::Plaintext,
                    AcquisitionParserArg::Markdown => shiro_sdk::AcquisitionParser::Markdown,
                    AcquisitionParserArg::Pdf => shiro_sdk::AcquisitionParser::Pdf,
                },
                *max_bytes,
                *timeout_ms,
                *max_redirects,
                *allow_http,
            )
        }

        Some(Commands::Ingest {
            dirs,
            max_files,
            follow,
            parser,
        }) => {
            let home = resolve_home(cli)?;
            commands::ingest::run(&home, dirs, *max_files, *follow, parser)
        }

        Some(Commands::Search {
            query,
            mode,
            limit,
            expand,
            max_blocks,
            max_chars,
            rerank,
            tag,
            concept,
            doc,
        }) => {
            let home = resolve_home(cli)?;
            let m = match mode {
                SearchModeArg::Hybrid => commands::search::SearchMode::Hybrid,
                SearchModeArg::Bm25 => commands::search::SearchMode::Bm25,
                SearchModeArg::Vector => commands::search::SearchMode::Vector,
            };
            let filters = shiro_sdk::SearchFilters {
                tags: tag.iter().cloned().collect(),
                concept_ids: concept.iter().cloned().collect(),
                document_ids: doc.iter().cloned().collect(),
            };
            commands::search::run(
                &home,
                query,
                m,
                *limit,
                *expand,
                *max_blocks,
                *max_chars,
                *rerank,
                filters,
            )
        }

        Some(Commands::SearchPack {
            queries,
            mode,
            per_query_limit,
            limit,
            include_content,
            rerank,
            tags,
            concept_ids,
            document_ids,
        }) => {
            let home = resolve_home(cli)?;
            commands::search_pack::run(
                &home,
                commands::search_pack::SearchPackOptions {
                    queries,
                    mode: match mode {
                        SearchModeArg::Hybrid => commands::search::SearchMode::Hybrid,
                        SearchModeArg::Bm25 => commands::search::SearchMode::Bm25,
                        SearchModeArg::Vector => commands::search::SearchMode::Vector,
                    },
                    per_query_limit: *per_query_limit,
                    global_limit: *limit,
                    include_content: *include_content,
                    rerank: *rerank,
                    tags,
                    concept_ids,
                    document_ids,
                },
            )
        }
        Some(Commands::Read { id, view, page }) => {
            let home = resolve_home(cli)?;
            let m = if page.is_some() {
                commands::read::ReadMode::Page
            } else {
                match view {
                    ReadView::Text => commands::read::ReadMode::Text,
                    ReadView::Blocks => commands::read::ReadMode::Blocks,
                    ReadView::Outline => commands::read::ReadMode::Outline,
                }
            };
            commands::read::run(&home, id, m, *page)
        }

        Some(Commands::Explain { result_id }) => {
            let home = resolve_home(cli)?;
            commands::explain::run(&home, result_id)
        }

        Some(Commands::List {
            limit,
            tag,
            concept,
        }) => {
            let home = resolve_home(cli)?;
            commands::list::run(
                &home,
                *limit,
                shiro_sdk::SearchFilters {
                    tags: tag.iter().cloned().collect(),
                    concept_ids: concept.iter().cloned().collect(),
                    document_ids: Vec::new(),
                },
            )
        }

        Some(Commands::Remove { id, purge }) => {
            let home = resolve_home(cli)?;
            commands::remove::run(&home, id, *purge)
        }

        Some(Commands::Doctor { verify_vector }) => {
            let home = resolve_home(cli)?;
            commands::doctor::run(&home, *verify_vector)
        }

        Some(Commands::Config { action }) => {
            let home = resolve_home(cli)?;
            match action {
                ConfigAction::Show => commands::config::run_show(&home),
                ConfigAction::Get { key } => commands::config::run_get(&home, key),
                ConfigAction::Set { key, value } => commands::config::run_set(&home, key, value),
            }
        }

        Some(Commands::Capabilities) => {
            let home = resolve_home(cli)?;
            commands::capabilities::run(&home)
        }

        Some(Commands::Mcp { allow_writes }) => {
            let home = resolve_home(cli)?;
            commands::mcp::run(home, *allow_writes)
        }

        Some(Commands::Taxonomy { action }) => {
            let home = resolve_home(cli)?;
            match action {
                TaxonomyAction::Add {
                    scheme,
                    label,
                    alt_labels,
                    definition,
                } => commands::taxonomy::run_add(
                    &home,
                    scheme,
                    label,
                    alt_labels.as_deref(),
                    definition.as_deref(),
                ),
                TaxonomyAction::List { limit } => commands::taxonomy::run_list(&home, *limit),
                TaxonomyAction::Search { query, limit } => {
                    commands::taxonomy::run_search(&home, query, *limit)
                }
                TaxonomyAction::Browse {
                    root,
                    max_depth,
                    max_nodes,
                } => commands::taxonomy::run_browse(&home, root.as_deref(), *max_depth, *max_nodes),
                TaxonomyAction::Relations { concept_id } => {
                    commands::taxonomy::run_relations(&home, concept_id)
                }
                TaxonomyAction::Relate {
                    from_concept_id,
                    to_concept_id,
                    kind,
                } => commands::taxonomy::run_relate(
                    &home,
                    from_concept_id,
                    to_concept_id,
                    match kind {
                        TaxonomyRelationKindArg::Broader => shiro_core::SkosRelation::Broader,
                        TaxonomyRelationKindArg::Narrower => shiro_core::SkosRelation::Narrower,
                        TaxonomyRelationKindArg::Related => shiro_core::SkosRelation::Related,
                    },
                ),
                TaxonomyAction::Assign {
                    doc_id,
                    concept_id,
                    confidence,
                    source,
                } => commands::taxonomy::run_assign(&home, doc_id, concept_id, *confidence, source),
                TaxonomyAction::Import { file } => commands::taxonomy::run_import(&home, file),
            }
        }

        Some(Commands::Reindex) => {
            let home = resolve_home(cli)?;
            commands::reindex::run(&home)
        }

        Some(Commands::Reprocess {
            document_ids,
            parser,
            target,
            execute,
            include_vector,
            resume_manifest_id,
            max_documents,
            max_source_bytes,
            max_model_calls,
            embedding_batch_size,
        }) => {
            let home = resolve_home(cli)?;
            commands::reprocess::run(
                &home,
                commands::reprocess::ReprocessOptions {
                    document_ids,
                    parser_name: parser,
                    target: match target {
                        ReprocessTargetArg::Parse => shiro_sdk::ReprocessTarget::Parse,
                        ReprocessTargetArg::Derived => shiro_sdk::ReprocessTarget::Derived,
                        ReprocessTargetArg::All => shiro_sdk::ReprocessTarget::All,
                    },
                    execute: *execute,
                    include_vector: *include_vector,
                    resume_manifest_id: resume_manifest_id.as_deref(),
                    max_documents: *max_documents,
                    max_source_bytes: *max_source_bytes,
                    max_model_calls: *max_model_calls,
                    embedding_batch_size: *embedding_batch_size,
                },
            )
        }

        Some(Commands::Benchmark {
            manifest,
            warmup_runs,
            measured_runs,
        }) => {
            let home = resolve_home(cli)?;
            commands::benchmark::run(&home, manifest, *warmup_runs, *measured_runs)
        }

        Some(Commands::Completions { .. }) => {
            // Handled in main() before dispatch — should never reach here.
            unreachable!("completions handled before dispatch")
        }

        Some(Commands::Enrich { id }) => {
            let home = resolve_home(cli)?;
            commands::enrich::run(&home, id)
        }

        Some(Commands::EnrichModel { action }) => {
            let home = resolve_home(cli)?;
            match action {
                ModelEnrichmentAction::Propose { file } => {
                    commands::model_enrichment::run_propose(&home, file)
                }
                ModelEnrichmentAction::Resolve {
                    proposal_id,
                    action,
                    actor,
                    approval,
                } => commands::model_enrichment::run_resolve(
                    &home,
                    proposal_id,
                    match action {
                        ModelEnrichmentResolutionArg::Promote => {
                            shiro_sdk::ModelEnrichmentResolutionAction::Promote
                        }
                        ModelEnrichmentResolutionArg::Reject => {
                            shiro_sdk::ModelEnrichmentResolutionAction::Reject
                        }
                    },
                    actor,
                    approval,
                ),
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
        Some(Commands::AcquireUrl { .. }) => "shiro acquire-url",
        Some(Commands::Ingest { .. }) => "shiro ingest",
        Some(Commands::Search { .. }) => "shiro search",
        Some(Commands::SearchPack { .. }) => "shiro search-pack",
        Some(Commands::Read { .. }) => "shiro read",
        Some(Commands::Explain { .. }) => "shiro explain",
        Some(Commands::List { .. }) => "shiro list",
        Some(Commands::Remove { .. }) => "shiro remove",
        Some(Commands::Doctor { .. }) => "shiro doctor",
        Some(Commands::Config { .. }) => "shiro config",
        Some(Commands::Capabilities) => "shiro capabilities",
        Some(Commands::Mcp { .. }) => "shiro mcp",
        Some(Commands::Taxonomy { .. }) => "shiro taxonomy",
        Some(Commands::Reindex) => "shiro reindex",
        Some(Commands::Reprocess { .. }) => "shiro reprocess",
        Some(Commands::Benchmark { .. }) => "shiro benchmark",
        Some(Commands::Completions { .. }) => "shiro completions",
        Some(Commands::Enrich { .. }) => "shiro enrich",
        Some(Commands::EnrichModel { .. }) => "shiro enrich-model",
    }
}

fn suggest_fix(err: &ShiroError) -> Option<&'static str> {
    match err {
        ShiroError::LockBusy { .. } => {
            Some("Another shiro process may be running. Wait or run `shiro doctor`.")
        }
        ShiroError::StoreCorrupt { .. } => {
            Some("Database may be corrupt. Run `shiro doctor` or re-init with `shiro init`.")
        }
        ShiroError::ParsePdf { .. } => Some("Ensure the file is a valid PDF."),
        ShiroError::ParseExternal { .. } => Some(
            "External parser failed. Check that the parser binary is installed and accessible.",
        ),
        ShiroError::Config { .. } => Some("Check SHIRO_HOME or run `shiro init`."),
        _ => None,
    }
}

fn recovery_actions(err: &ShiroError) -> Vec<NextAction> {
    match err {
        ShiroError::StoreCorrupt { .. } => {
            vec![NextAction::simple("shiro doctor", "Run diagnostics")]
        }
        ShiroError::LockBusy { .. } => {
            vec![NextAction::simple("shiro doctor", "Check for stale locks")]
        }
        _ => vec![NextAction::simple("shiro doctor", "Run diagnostics")],
    }
}
