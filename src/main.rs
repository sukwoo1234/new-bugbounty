use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand};

mod common;
mod coverage;
mod dashboard_charts;
mod dashboard_data;
mod json_utils;
mod metrics;
mod mutate;
mod report;
mod retention;
mod run;
mod seed;
mod target;
mod triage;
mod ui;

use common::AppPaths;
use dashboard_data::collect_dashboard_snapshot;
use run::RunBackend;
use target::TargetKind;

const E_CONFIG_PREPARE: &str = "E1001";
const E_PREPARE_TARGET: &str = "E2001";
const E_RUN_PIPELINE: &str = "E3001";
const E_HARNESS_EXEC: &str = "E3002";
const E_TRIAGE_PIPELINE: &str = "E4001";
const E_REPORT_PIPELINE: &str = "E5001";
const E_UI_SERVER: &str = "E6001";

#[derive(Parser)]
#[command(name = "tool", version, about = "Bug bounty fuzzing platform CLI")]
struct Cli {
    /// Data directory (default: ./data)
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// Seeds directory (default: ./seeds)
    #[arg(long, default_value = "./seeds")]
    seeds_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunArgs),
    Triage(TriageArgs),
    Report(ReportArgs),
    Seed(SeedArgs),
    Dashboard(DashboardArgs),
    Coverage(CoverageArgs),
    UiServe(UiServeArgs),
    List(ListArgs),
    Show(ShowArgs),
    Export(ExportArgs),
    PrepareTarget(PrepareTargetArgs),
    Harness(HarnessArgs),
    Mutate(MutateArgs),
}

#[derive(Args)]
struct RunArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Run backend: local-harness | aflpp | libfuzzer
    #[arg(long, value_enum, default_value_t = RunBackend::LocalHarness)]
    backend: RunBackend,

    /// Local development mode: default corpus_dir becomes seeds/<target>
    #[arg(long, default_value_t = false)]
    local: bool,

    /// Corpus directory (default: seeds_dir)
    #[arg(long)]
    corpus_dir: Option<PathBuf>,

    /// Parallel workers (default: 8)
    #[arg(long, default_value_t = 8)]
    workers: usize,

    /// Per-input timeout in seconds (default: 60)
    #[arg(long, default_value_t = 60)]
    timeout_sec: u64,

    /// Retry count on failure/timeout (default: 1)
    #[arg(long, default_value_t = 1)]
    restart_limit: u32,

    /// Max number of corpus files to process (default: all)
    #[arg(long)]
    max_jobs: Option<usize>,
}

#[derive(Args)]
struct TriageArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Input file to reproduce
    #[arg(long)]
    input: PathBuf,

    /// Reproduction attempts (default: 3)
    #[arg(long, default_value_t = 3)]
    repro_retries: u32,

    /// Per-attempt timeout in seconds (default: 60)
    #[arg(long, default_value_t = 60)]
    timeout_sec: u64,
}

#[derive(Args)]
struct ReportArgs {
    /// Add minimization metadata/artifacts without blocking report generation
    #[arg(long, default_value_t = false)]
    minimize: bool,
}

#[derive(Args)]
struct SeedArgs {
    #[command(subcommand)]
    command: SeedCommands,
}

#[derive(Subcommand)]
enum SeedCommands {
    Sync(SeedSyncArgs),
    Stats(SeedStatsArgs),
}

#[derive(Args)]
struct SeedSyncArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Source directory to collect seeds from
    #[arg(long)]
    from: PathBuf,

    /// Destination directory (default: seeds/<target>)
    #[arg(long)]
    to: Option<PathBuf>,

    /// Validate each candidate with `tool harness` before copy
    #[arg(long, default_value_t = false)]
    harness_filter: bool,
}

#[derive(Args)]
struct SeedStatsArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Seed directory to inspect (default: seeds/<target>)
    #[arg(long)]
    dir: Option<PathBuf>,
}

#[derive(Args)]
struct ListArgs {
    /// Resource kind: all | runs | triages | reports | coverage
    #[arg(long, default_value = "all")]
    kind: String,

    /// Limit per kind (default 20)
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Args)]
struct DashboardArgs {
    /// Output format: json | html
    #[arg(long, value_enum, default_value_t = DashboardFormat::Json)]
    format: DashboardFormat,

    /// Output file path (required for html)
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct CoverageArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Corpus directory (default: seeds/<target>)
    #[arg(long)]
    corpus_dir: Option<PathBuf>,

    /// Per-input timeout in seconds (default: 30)
    #[arg(long, default_value_t = 30)]
    timeout_sec: u64,

    /// Max number of corpus files to process (default: all)
    #[arg(long)]
    max_jobs: Option<usize>,
}

#[derive(Args)]
struct UiServeArgs {
    /// Bind address for UI server (default: 127.0.0.1:8787)
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
}

#[derive(clap::ValueEnum, Clone, Debug, PartialEq, Eq)]
enum DashboardFormat {
    #[value(name = "json")]
    Json,
    #[value(name = "html")]
    Html,
}

#[derive(Args)]
struct ShowArgs {
    /// Result ID to show
    id: String,
}

#[derive(Args)]
struct ExportArgs {
    /// Result ID to export
    id: String,
}

#[derive(Args)]
struct PrepareTargetArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Source URL to official release asset
    #[arg(long)]
    source_url: Option<String>,

    /// Override pinned version
    #[arg(long)]
    version: Option<String>,
}

#[derive(Args)]
struct HarnessArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Input file path
    #[arg(long)]
    input: PathBuf,
}

#[derive(Args)]
struct MutateArgs {
    /// Target type: gguf | onnx | safetensors
    #[arg(long, value_enum)]
    target: TargetKind,

    /// Input seed file path for single-file mode
    #[arg(long)]
    input: Option<PathBuf>,

    /// Output mutated file path for single-file mode
    #[arg(long)]
    out: Option<PathBuf>,

    /// Input seed directory for batch mode
    #[arg(long)]
    input_dir: Option<PathBuf>,

    /// Output directory for batch mode
    #[arg(long)]
    out_dir: Option<PathBuf>,

    /// Number of mutated files to generate in batch mode
    #[arg(long, default_value_t = 1)]
    count: usize,

    /// Deterministic mutation seed
    #[arg(long, default_value_t = 1)]
    seed: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let app_paths = match AppPaths::prepare(&cli.data_dir, &cli.seeds_dir) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("[{E_CONFIG_PREPARE}] config error: {err}");
            return ExitCode::from(2);
        }
    };

    match cli.command {
        Commands::Run(args) => {
            if let Err(err) = run::run_fuzz_pipeline(
                &app_paths,
                &args.target,
                &args.backend,
                args.local,
                args.corpus_dir.as_deref(),
                args.workers,
                args.timeout_sec,
                args.restart_limit,
                args.max_jobs,
            ) {
                eprintln!("[{E_RUN_PIPELINE}] run error: {err}");
                return ExitCode::from(5);
            }
        }
        Commands::Triage(args) => {
            if let Err(err) = triage::run_triage_pipeline(
                &app_paths,
                &args.target,
                &args.input,
                args.repro_retries,
                args.timeout_sec,
            ) {
                eprintln!("[{E_TRIAGE_PIPELINE}] triage error: {err}");
                return ExitCode::from(6);
            }
        }
        Commands::Report(args) => {
            if let Err(err) = report::run_report_pipeline(&app_paths, args.minimize) {
                eprintln!("[{E_REPORT_PIPELINE}] report error: {err}");
                return ExitCode::from(7);
            }
        }
        Commands::Seed(args) => {
            let result = match args.command {
                SeedCommands::Sync(sync) => seed::run_seed_sync(
                    &app_paths,
                    &sync.target,
                    &sync.from,
                    sync.to.as_deref(),
                    sync.harness_filter,
                ),
                SeedCommands::Stats(stats) => {
                    seed::run_seed_stats(&app_paths, &stats.target, stats.dir.as_deref())
                }
            };
            if let Err(err) = result {
                eprintln!("[{E_CONFIG_PREPARE}] seed error: {err}");
                return ExitCode::from(2);
            }
        }
        Commands::Dashboard(args) => {
            let result: Result<(), String> = (|| {
                let snap = collect_dashboard_snapshot(&app_paths)?;
                match args.format {
                    DashboardFormat::Json => {
                        println!("{}", ui::dashboard::render_dashboard_json(&snap));
                    }
                    DashboardFormat::Html => {
                        let Some(out) = args.out.as_ref() else {
                            return Err("html format requires --out <path>".to_string());
                        };
                        let html = ui::dashboard::render_dashboard_html(&snap);
                        if let Some(parent) = out.parent() {
                            if !parent.as_os_str().is_empty() {
                                fs::create_dir_all(parent).map_err(|e| {
                                    format!(
                                        "failed to create dashboard dir '{}': {e}",
                                        parent.display()
                                    )
                                })?;
                            }
                        }
                        fs::write(out, html).map_err(|e| {
                            format!("failed to write dashboard html '{}': {e}", out.display())
                        })?;
                        println!("[dashboard] done");
                        println!("format: html");
                        println!("out: {}", out.display());
                    }
                }
                Ok(())
            })();
            if let Err(err) = result {
                eprintln!("[{E_CONFIG_PREPARE}] dashboard error: {err}");
                return ExitCode::from(2);
            }
        }
        Commands::Coverage(args) => {
            if let Err(err) = coverage::run_coverage_job(
                &app_paths,
                &args.target,
                args.corpus_dir.as_deref(),
                args.timeout_sec,
                args.max_jobs,
            ) {
                eprintln!("[{E_CONFIG_PREPARE}] coverage error: {err}");
                return ExitCode::from(2);
            }
        }
        Commands::UiServe(args) => {
            if let Err(err) = ui::server::run_ui_server(&app_paths, &args.bind) {
                eprintln!("[{E_UI_SERVER}] ui-serve error: {err}");
                return ExitCode::from(8);
            }
        }
        Commands::List(args) => {
            let kinds: Vec<&str> = if args.kind == "all" {
                vec!["runs", "triages", "reports", "coverage"]
            } else {
                vec![args.kind.as_str()]
            };
            let mut had_error = false;
            for kind in kinds {
                match dashboard_data::list_recent_ids(&app_paths, kind, args.limit) {
                    Ok(ids) => {
                        println!("[{kind}]");
                        if ids.is_empty() {
                            println!("  (none)");
                        } else {
                            for id in &ids {
                                println!("  {id}");
                            }
                        }
                        println!();
                    }
                    Err(err) => {
                        eprintln!("[{E_CONFIG_PREPARE}] list error ({kind}): {err}");
                        had_error = true;
                    }
                }
            }
            if had_error {
                return ExitCode::from(2);
            }
        }
        Commands::Show(args) => {
            print_stub_with_id("show", &app_paths.data_dir, &app_paths.seeds_dir, &args.id);
        }
        Commands::Export(args) => {
            print_stub_with_id(
                "export",
                &app_paths.data_dir,
                &app_paths.seeds_dir,
                &args.id,
            );
        }
        Commands::PrepareTarget(args) => {
            if let Err(err) =
                target::prepare_target(&app_paths, &args.target, args.version, args.source_url)
            {
                eprintln!("[{E_PREPARE_TARGET}] prepare-target error: {err}");
                return ExitCode::from(3);
            }
        }
        Commands::Harness(args) => {
            if let Err(err) = target::run_harness(&args.target, &args.input) {
                eprintln!("[{E_HARNESS_EXEC}] harness error: {err}");
                return ExitCode::from(4);
            }
        }
        Commands::Mutate(args) => {
            if let Err(err) = mutate::run_mutate_pipeline(
                &args.target,
                args.input.as_deref(),
                args.out.as_deref(),
                args.input_dir.as_deref(),
                args.out_dir.as_deref(),
                args.count,
                args.seed,
            ) {
                eprintln!("[{E_CONFIG_PREPARE}] mutate error: {err}");
                return ExitCode::from(2);
            }
        }
    }

    ExitCode::SUCCESS
}

fn print_stub_with_id(command: &str, data_dir: &Path, seeds_dir: &Path, id: &str) {
    println!("[{}] not implemented yet", command);
    println!("id: {}", id);
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}
