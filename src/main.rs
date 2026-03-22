use std::{
    collections::{HashSet, VecDeque},
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    process::ExitCode,
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Args, Parser, Subcommand};
use metrics::MetricEvent;

mod metrics;
mod report;
mod retention;
mod ui;

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
struct ReportArgs {}

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
struct ListArgs {}

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

#[derive(Clone, Debug)]
pub(crate) struct DashboardSnapshot {
    pub(crate) generated_at: u64,
    pub(crate) data_dir: String,
    pub(crate) seeds_dir: String,
    pub(crate) runs_count: usize,
    pub(crate) triage_count: usize,
    pub(crate) report_count: usize,
    pub(crate) latest_run: String,
    pub(crate) latest_triage: String,
    pub(crate) latest_report: String,
    pub(crate) metrics_exists: bool,
    pub(crate) new_paths_per_hour: String,
    pub(crate) new_crashes_per_hour: String,
    pub(crate) valid_crash_ratio: String,
    pub(crate) global_error_rate_5m: String,
    pub(crate) latest_valid_triage: String,
    pub(crate) latest_valid_input: String,
    pub(crate) latest_valid_signature: String,
    pub(crate) latest_valid_summary: String,
    pub(crate) latest_valid_report: String,
    pub(crate) recent_triage_ids: Vec<String>,
    pub(crate) recent_report_ids: Vec<String>,
    pub(crate) recent_coverage_ids: Vec<String>,
    pub(crate) seeds_onnx_count: usize,
    pub(crate) seeds_gguf_count: usize,
    pub(crate) seeds_safetensors_count: usize,
    pub(crate) seeds_total_count: usize,
    pub(crate) coverage_count: usize,
    pub(crate) latest_coverage: String,
    pub(crate) latest_coverage_summary: String,
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

#[derive(clap::ValueEnum, Clone, Debug)]
enum TargetKind {
    #[value(name = "gguf")]
    Gguf,
    #[value(name = "onnx")]
    Onnx,
    #[value(name = "safetensors")]
    Safetensors,
}

#[derive(clap::ValueEnum, Clone, Debug, PartialEq, Eq)]
enum RunBackend {
    #[value(name = "local-harness")]
    LocalHarness,
    #[value(name = "aflpp")]
    Aflpp,
    #[value(name = "libfuzzer")]
    Libfuzzer,
}

struct EngineAdapter {
    backend_label: &'static str,
    cmd_env: &'static str,
}

struct TargetAdapter {
    target_label: &'static str,
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
            if let Err(err) = run_fuzz_pipeline(&app_paths, &args) {
                eprintln!("[{E_RUN_PIPELINE}] run error: {err}");
                return ExitCode::from(5);
            }
        }
        Commands::Triage(args) => {
            if let Err(err) = run_triage_pipeline(&app_paths, &args) {
                eprintln!("[{E_TRIAGE_PIPELINE}] triage error: {err}");
                return ExitCode::from(6);
            }
        }
        Commands::Report(_args) => {
            if let Err(err) = run_report_pipeline(&app_paths) {
                eprintln!("[{E_REPORT_PIPELINE}] report error: {err}");
                return ExitCode::from(7);
            }
        }
        Commands::Seed(args) => {
            if let Err(err) = run_seed_tool(&app_paths, &args) {
                eprintln!("[{E_CONFIG_PREPARE}] seed error: {err}");
                return ExitCode::from(2);
            }
        }
        Commands::Dashboard(args) => {
            if let Err(err) = run_dashboard_snapshot(&app_paths, &args) {
                eprintln!("[{E_CONFIG_PREPARE}] dashboard error: {err}");
                return ExitCode::from(2);
            }
        }
        Commands::Coverage(args) => {
            if let Err(err) = run_coverage_job(&app_paths, &args) {
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
        Commands::List(_args) => {
            print_stub("list", &app_paths.data_dir, &app_paths.seeds_dir);
        }
        Commands::Show(args) => {
            print_stub_with_id("show", &app_paths.data_dir, &app_paths.seeds_dir, &args.id);
        }
        Commands::Export(args) => {
            print_stub_with_id("export", &app_paths.data_dir, &app_paths.seeds_dir, &args.id);
        }
        Commands::PrepareTarget(args) => {
            if let Err(err) = prepare_target(&app_paths, &args) {
                eprintln!("[{E_PREPARE_TARGET}] prepare-target error: {err}");
                return ExitCode::from(3);
            }
        }
        Commands::Harness(args) => {
            if let Err(err) = run_harness(&args) {
                eprintln!("[{E_HARNESS_EXEC}] harness error: {err}");
                return ExitCode::from(4);
            }
        }
    }

    ExitCode::SUCCESS
}

pub(crate) struct AppPaths {
    pub(crate) data_dir: PathBuf,
    pub(crate) seeds_dir: PathBuf,
}

#[derive(Clone, Copy)]
struct TargetPreset {
    name: &'static str,
    default_version: &'static str,
    default_url: &'static str,
    official_repo_prefix: &'static str,
}

struct TargetMeta {
    schema_version: &'static str,
    target: String,
    version: String,
    source_url: String,
    source_kind: &'static str,
    downloaded_file: String,
    downloaded_sha256: String,
    downloaded_size_bytes: u64,
}

struct HarnessReport {
    target: &'static str,
    input: String,
    parser_step: String,
    core_path_step: String,
    library_step: String,
    external_step: String,
}

struct LibraryConnectResult {
    step: String,
    connected: bool,
}

#[derive(Clone)]
struct RunJob {
    id: usize,
    input: PathBuf,
}

#[derive(Default)]
struct RunStats {
    total: usize,
    success: usize,
    failed: usize,
    timeout: usize,
    retries: usize,
}

enum HarnessExecResult {
    Success(String),
    Failed(String),
    Timeout(String),
}

struct TriageAttempt {
    attempt: u32,
    result: String,
    signature_top3: Vec<String>,
}

impl AppPaths {
    fn prepare(data_dir: &Path, seeds_dir: &Path) -> Result<Self, String> {
        ensure_directory(data_dir)
            .map_err(|e| format!("failed to create data dir '{}': {e}", data_dir.display()))?;
        ensure_data_layout(data_dir)
            .map_err(|e| format!("failed to create data layout in '{}': {e}", data_dir.display()))?;
        ensure_directory(seeds_dir)
            .map_err(|e| format!("failed to create seeds dir '{}': {e}", seeds_dir.display()))?;

        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            seeds_dir: seeds_dir.to_path_buf(),
        })
    }
}

fn prepare_target(app_paths: &AppPaths, args: &PrepareTargetArgs) -> Result<(), String> {
    let preset = preset_for_target(&args.target);
    let version = args
        .version
        .clone()
        .unwrap_or_else(|| preset.default_version.to_string());
    let source_url = args
        .source_url
        .clone()
        .unwrap_or_else(|| preset.default_url.to_string());

    validate_official_source(&source_url, preset.official_repo_prefix)?;

    let file_name = download_file_name(&source_url);

    let target_root = app_paths
        .data_dir
        .join("targets")
        .join(preset.name)
        .join(&version);
    let source_dir = target_root.join("source");
    fs::create_dir_all(&source_dir)
        .map_err(|e| format!("failed to create '{}': {e}", source_dir.display()))?;

    let file_path = source_dir.join(&file_name);
    download_file(&source_url, &file_path)?;
    let sha256 = sha256_file(&file_path)?;
    let size_bytes = fs::metadata(&file_path)
        .map_err(|e| format!("failed to read metadata '{}': {e}", file_path.display()))?
        .len();

    let meta = TargetMeta {
        schema_version: "1.0",
        target: preset.name.to_string(),
        version: version.clone(),
        source_url,
        source_kind: "official_release",
        downloaded_file: file_path.display().to_string(),
        downloaded_sha256: sha256,
        downloaded_size_bytes: size_bytes,
    };

    let meta_path = target_root.join("meta.json");
    let meta_json = render_meta_json(&meta);
    fs::write(&meta_path, meta_json)
        .map_err(|e| format!("failed to write '{}': {e}", meta_path.display()))?;

    println!("[prepare-target] done");
    println!("target: {}", preset.name);
    println!("version: {version}");
    println!("file: {}", file_path.display());
    println!("sha256: {}", meta.downloaded_sha256);
    println!("meta: {}", meta_path.display());

    Ok(())
}

fn preset_for_target(target: &TargetKind) -> TargetPreset {
    match target {
        TargetKind::Gguf => TargetPreset {
            name: "llama.cpp",
            default_version: "b7921",
            default_url: "https://github.com/ggml-org/llama.cpp/archive/refs/tags/b7921.tar.gz",
            official_repo_prefix: "/ggml-org/llama.cpp/",
        },
        TargetKind::Onnx => TargetPreset {
            name: "onnxruntime",
            default_version: "v1.23.2",
            default_url: "https://github.com/microsoft/onnxruntime/archive/refs/tags/v1.23.2.tar.gz",
            official_repo_prefix: "/microsoft/onnxruntime/",
        },
        TargetKind::Safetensors => TargetPreset {
            name: "safetensors",
            default_version: "v0.7.0",
            default_url: "https://github.com/huggingface/safetensors/archive/refs/tags/v0.7.0.tar.gz",
            official_repo_prefix: "/huggingface/safetensors/",
        },
    }
}

fn run_fuzz_pipeline(app_paths: &AppPaths, args: &RunArgs) -> Result<(), String> {
    if args.backend != RunBackend::LocalHarness {
        return run_engine_backend(app_paths, args);
    }

    let corpus_dir = match &args.corpus_dir {
        Some(path) => path.clone(),
        None if args.local => app_paths.seeds_dir.join(target_label(&args.target)),
        None => app_paths.seeds_dir.clone(),
    };
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!(
            "corpus_dir is invalid: {}",
            corpus_dir.display()
        ));
    }

    // corpus reload: v1은 시작 시 코퍼스를 스냅샷으로 고정한다.
    // 재실행 루프 사이 신규 파일 반영은 차기 단계에서 "루프 간 재스캔"으로 확장한다.
    let mut inputs = collect_corpus_inputs(&corpus_dir, &args.target)?;
    if inputs.is_empty() {
        return Err(format!(
            "no input files found for target '{}' in {}",
            target_label(&args.target),
            corpus_dir.display()
        ));
    }
    if let Some(max_jobs) = args.max_jobs {
        inputs.truncate(max_jobs);
    }

    let run_id = now_unix_millis();
    let run_dir = app_paths
        .data_dir
        .join("runs")
        .join(format!("run-{run_id}"));
    let logs_dir = run_dir.join("logs");
    fs::create_dir_all(&logs_dir)
        .map_err(|e| format!("failed to create run log dir '{}': {e}", logs_dir.display()))?;

    let jobs = inputs
        .into_iter()
        .enumerate()
        .map(|(id, input)| RunJob { id, input })
        .collect::<Vec<_>>();
    let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));
    let stats = Arc::new(Mutex::new(RunStats {
        total: queue.lock().map_err(|_| "queue lock poisoned")?.len(),
        ..RunStats::default()
    }));

    let workers = args.workers.max(1).min(
        queue
            .lock()
            .map_err(|_| "queue lock poisoned")?
            .len()
            .max(1),
    );
    let timeout_available = command_exists("timeout");

    println!("[run] start");
    println!("target: {}", target_label(&args.target));
    println!("backend: {}", run_backend_label(&args.backend));
    println!("local_mode: {}", args.local);
    println!("corpus_dir: {}", corpus_dir.display());
    println!("workers: {workers}");
    println!("timeout_sec: {}", args.timeout_sec);
    println!("restart_limit: {}", args.restart_limit);
    println!("run_dir: {}", run_dir.display());

    let mut handles = Vec::new();
    for _worker_id in 0..workers {
        let queue = Arc::clone(&queue);
        let stats = Arc::clone(&stats);
        let logs_dir = logs_dir.clone();
        let target = args.target.clone();
        let timeout_sec = args.timeout_sec;
        let restart_limit = args.restart_limit;

        handles.push(thread::spawn(move || {
            loop {
                let job = {
                    let mut guard = match queue.lock() {
                        Ok(g) => g,
                        Err(_) => return Err("queue lock poisoned".to_string()),
                    };
                    guard.pop_front()
                };

                let Some(job) = job else {
                    break;
                };

                let (result, retries_used) = run_job_with_retry(
                    &job,
                    &target,
                    timeout_sec,
                    restart_limit,
                    timeout_available,
                    &logs_dir,
                )?;

                let mut s = stats
                    .lock()
                    .map_err(|_| "stats lock poisoned".to_string())?;
                s.retries += retries_used;
                match result {
                    HarnessExecResult::Success(_) => s.success += 1,
                    HarnessExecResult::Failed(_) => s.failed += 1,
                    HarnessExecResult::Timeout(_) => s.timeout += 1,
                }
            }
            Ok(())
        }));
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("worker thread panicked".to_string()),
        }
    }

    // zombie fencing: 현재 구현은 파일 큐 대신 in-memory pop_front 단일 소유권으로 중복 처리 write를 방지한다.
    // file-queue 전환 시에는 결과 쓰기 전에 processing/<job_id> 존재 확인을 강제한다.
    let s = stats.lock().map_err(|_| "stats lock poisoned")?;
    let status_path = write_run_status(
        &run_dir,
        run_id,
        &args.target,
        RunStatusCounts {
            total: s.total,
            success: s.success,
            failed: s.failed,
            timeout: s.timeout,
            retries: s.retries,
        },
        workers,
        args.timeout_sec,
        args.restart_limit,
    )?;

    println!("[run] done");
    println!("success: {}", s.success);
    println!("failed: {}", s.failed);
    println!("timeout: {}", s.timeout);
    println!("retries: {}", s.retries);
    println!("status: {}", status_path.display());

    record_metrics_event(
        app_paths,
        MetricEvent {
            ts: now_unix(),
            kind: "run",
            total: s.total as u64,
            errors: (s.failed + s.timeout) as u64,
            // v1: success count를 신규 경로 proxy로 수집
            new_paths: s.success as u64,
            new_crashes: 0,
            valid_crashes: 0,
            total_crashes: 0,
        },
    )?;
    Ok(())
}

fn run_engine_backend(app_paths: &AppPaths, args: &RunArgs) -> Result<(), String> {
    if args.workers == 0 {
        return Err("workers must be >= 1".to_string());
    }

    let corpus_dir = match &args.corpus_dir {
        Some(path) => path.clone(),
        None if args.local => app_paths.seeds_dir.join(target_label(&args.target)),
        None => app_paths.seeds_dir.clone(),
    };
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!(
            "corpus_dir is invalid: {}",
            corpus_dir.display()
        ));
    }

    let run_id = now_unix_millis();
    let run_dir = app_paths
        .data_dir
        .join("runs")
        .join(format!("run-{run_id}"));
    let logs_dir = run_dir.join("logs");
    fs::create_dir_all(&logs_dir)
        .map_err(|e| format!("failed to create run dir '{}': {e}", run_dir.display()))?;

    println!("[run] start");
    println!("target: {}", target_label(&args.target));
    println!("backend: {}", run_backend_label(&args.backend));
    println!("local_mode: {}", args.local);
    println!("corpus_dir: {}", corpus_dir.display());
    println!("workers: {}", args.workers);
    println!("timeout_sec: {}", args.timeout_sec);
    println!("restart_limit: {}", args.restart_limit);
    println!("run_dir: {}", run_dir.display());

    let mut success = 0usize;
    let mut failed = 0usize;
    let mut timeout = 0usize;

    for worker_id in 1..=args.workers {
        let worker_log_path = logs_dir.join(format!("backend-engine-w{worker_id}.log"));
        let engine_cmd =
            build_engine_command(args, &corpus_dir, &run_dir, worker_id, &worker_log_path)?;
        println!("backend_engine_cmd[w{worker_id}]: {engine_cmd}");

        let mut cmd = command_with_core_dump_off("bash");
        cmd.arg("-lc").arg(&engine_cmd);
        let output = cmd
            .output()
            .map_err(|e| format!("failed to execute backend engine command for worker {worker_id}: {e}"))?;
        let log_body = format!(
            "worker_id: {worker_id}\ncmd: {engine_cmd}\nexit_code: {:?}\n\n[stdout]\n{}\n\n[stderr]\n{}\n",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(&worker_log_path, log_body)
            .map_err(|e| format!("failed to write '{}': {e}", worker_log_path.display()))?;

        let exit_code = output.status.code().unwrap_or(1);
        if output.status.success() {
            success += 1;
        } else if exit_code == 124 {
            timeout += 1;
        } else {
            failed += 1;
        }
    }

    let status_path = write_run_status(
        &run_dir,
        run_id,
        &args.target,
        RunStatusCounts {
            total: args.workers,
            success,
            failed,
            timeout,
            retries: 0,
        },
        args.workers,
        args.timeout_sec,
        args.restart_limit,
    )?;

    println!("[run] done");
    println!("success: {success}");
    println!("failed: {failed}");
    println!("timeout: {timeout}");
    println!("retries: 0");
    println!("status: {}", status_path.display());

    record_metrics_event(
        app_paths,
        MetricEvent {
            ts: now_unix(),
            kind: "run",
            total: args.workers as u64,
            errors: (failed + timeout) as u64,
            new_paths: success as u64,
            new_crashes: 0,
            valid_crashes: 0,
            total_crashes: 0,
        },
    )?;

    if failed == 0 && timeout == 0 {
        Ok(())
    } else {
        Err(format!(
            "backend '{}' engine command failed (failed={}, timeout={}, run_dir={})",
            run_backend_label(&args.backend),
            failed,
            timeout,
            run_dir.display()
        ))
    }
}

fn build_engine_command(
    args: &RunArgs,
    corpus_dir: &Path,
    run_dir: &Path,
    worker_id: usize,
    worker_log_path: &Path,
) -> Result<String, String> {
    let engine = resolve_engine_adapter(&args.backend)?;
    let target = resolve_target_adapter(&args.target);
    let template = std::env::var(engine.cmd_env).map_err(|_| {
        format!(
            "{} is not set; provide backend command template. example: {}='echo run {{target}} {{corpus_dir}}; true'",
            engine.cmd_env, engine.cmd_env
        )
    })?;
    if template.trim().is_empty() {
        return Err(format!("{} is empty", engine.cmd_env));
    }

    let cmd = template
        .replace("{target}", target.target_label)
        .replace("{backend}", engine.backend_label)
        .replace("{corpus_dir}", &shell_escape(corpus_dir))
        .replace("{workers}", &args.workers.to_string())
        .replace("{worker_id}", &worker_id.to_string())
        .replace("{timeout_sec}", &args.timeout_sec.to_string())
        .replace("{restart_limit}", &args.restart_limit.to_string())
        .replace("{run_dir}", &shell_escape(run_dir))
        .replace("{workdir}", &shell_escape(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))))
        .replace("{worker_log}", &shell_escape(worker_log_path));

    Ok(cmd)
}

fn resolve_engine_adapter(backend: &RunBackend) -> Result<EngineAdapter, String> {
    match backend {
        RunBackend::Aflpp => Ok(EngineAdapter {
            backend_label: "aflpp",
            cmd_env: "TOOL_AFLPP_CMD",
        }),
        RunBackend::Libfuzzer => Ok(EngineAdapter {
            backend_label: "libfuzzer",
            cmd_env: "TOOL_LIBFUZZER_CMD",
        }),
        RunBackend::LocalHarness => {
            Err("internal error: local-harness should not use engine command".to_string())
        }
    }
}

fn resolve_target_adapter(target: &TargetKind) -> TargetAdapter {
    TargetAdapter {
        target_label: target_label(target),
    }
}

fn shell_escape(path: &Path) -> String {
    let s = path.display().to_string();
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn collect_corpus_inputs(corpus_dir: &Path, target: &TargetKind) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(corpus_dir)
        .map_err(|e| format!("failed to read corpus dir '{}': {e}", corpus_dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let keep = match target {
            TargetKind::Gguf => has_ext(&path, "gguf"),
            TargetKind::Onnx => has_ext(&path, "onnx"),
            TargetKind::Safetensors => has_ext(&path, "safetensors"),
        };
        if keep {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

fn has_ext(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

fn run_job_with_retry(
    job: &RunJob,
    target: &TargetKind,
    timeout_sec: u64,
    restart_limit: u32,
    timeout_available: bool,
    logs_dir: &Path,
) -> Result<(HarnessExecResult, usize), String> {
    let attempts = restart_limit + 1;
    let mut last = HarnessExecResult::Failed("not executed".to_string());
    let mut retries_used = 0usize;

    for attempt in 1..=attempts {
        let result = execute_harness_subprocess(job, target, timeout_sec, timeout_available)?;
        write_job_log(logs_dir, job, attempt, &result)?;
        match result {
            HarnessExecResult::Success(_) => return Ok((result, retries_used)),
            other => last = other,
        }
        if attempt < attempts {
            retries_used += 1;
        }
    }

    Ok((last, retries_used))
}

fn execute_harness_subprocess(
    job: &RunJob,
    target: &TargetKind,
    timeout_sec: u64,
    timeout_available: bool,
) -> Result<HarnessExecResult, String> {
    let exe = std::env::current_exe().map_err(|e| format!("failed to resolve current exe: {e}"))?;
    let target_name = target_label(target).to_string();
    let input = job.input.display().to_string();

    let mut cmd = if timeout_available {
        let mut c = command_with_core_dump_off("timeout");
        c.arg(format!("{}s", timeout_sec));
        c.arg(&exe);
        c
    } else {
        command_with_core_dump_off(&exe.display().to_string())
    };
    cmd.arg("harness")
        .arg("--target")
        .arg(&target_name)
        .arg("--input")
        .arg(&input)
        .env("OMP_NUM_THREADS", "1")
        .env("MKL_NUM_THREADS", "1")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("NUMEXPR_NUM_THREADS", "1")
        .env("VECLIB_MAXIMUM_THREADS", "1");

    let out = cmd
        .output()
        .map_err(|e| format!("failed to execute harness subprocess: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let summary = format!("stdout: {}\nstderr: {}", first_line(&stdout), first_line(&stderr));

    if timeout_available && out.status.code() == Some(124) {
        return Ok(HarnessExecResult::Timeout(summary));
    }
    if out.status.success() {
        return Ok(HarnessExecResult::Success(summary));
    }
    Ok(HarnessExecResult::Failed(summary))
}

fn write_job_log(
    logs_dir: &Path,
    job: &RunJob,
    attempt: u32,
    result: &HarnessExecResult,
) -> Result<(), String> {
    let path = logs_dir.join(format!("job-{:05}-attempt-{}.log", job.id, attempt));
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("failed to open '{}': {e}", path.display()))?;
    let (kind, summary) = match result {
        HarnessExecResult::Success(s) => ("success", s.as_str()),
        HarnessExecResult::Failed(s) => ("failed", s.as_str()),
        HarnessExecResult::Timeout(s) => ("timeout", s.as_str()),
    };
    let body = format!(
        "job_id: {}\ninput: {}\nattempt: {}\nresult: {}\n{}\n",
        job.id,
        job.input.display(),
        attempt,
        kind,
        summary
    );
    f.write_all(body.as_bytes())
        .map_err(|e| format!("failed to write '{}': {e}", path.display()))
}

fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

fn core_dump_off_env() -> String {
    let existing = std::env::var("ASAN_OPTIONS").unwrap_or_default();
    if existing.trim().is_empty() {
        "disable_coredump=1".to_string()
    } else if existing.contains("disable_coredump=") {
        existing
    } else {
        format!("{existing}:disable_coredump=1")
    }
}

fn command_with_core_dump_off(program: &str) -> Command {
    let mut cmd = if command_exists("prlimit") {
        let mut c = Command::new("prlimit");
        c.arg("--core=0").arg("--").arg(program);
        c
    } else {
        Command::new(program)
    };
    cmd.env("ASAN_OPTIONS", core_dump_off_env());
    cmd
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn run_triage_pipeline(app_paths: &AppPaths, args: &TriageArgs) -> Result<(), String> {
    if !args.input.exists() || !args.input.is_file() {
        return Err(format!("input is invalid: {}", args.input.display()));
    }
    if args.repro_retries == 0 {
        return Err("repro_retries must be >= 1".to_string());
    }

    let triage_id = now_unix();
    let triage_dir = app_paths
        .data_dir
        .join("triage")
        .join(format!("triage-{triage_id}"));
    fs::create_dir_all(&triage_dir)
        .map_err(|e| format!("failed to create triage dir '{}': {e}", triage_dir.display()))?;

    let timeout_available = command_exists("timeout");
    let mut attempts = Vec::new();

    for attempt in 1..=args.repro_retries {
        let exec = execute_triage_subprocess(
            &args.target,
            &args.input,
            args.timeout_sec,
            timeout_available,
        )?;
        let (result_label, merged_output) = match exec {
            HarnessExecResult::Success(s) => ("success".to_string(), s),
            HarnessExecResult::Failed(s) => ("failed".to_string(), s),
            HarnessExecResult::Timeout(s) => ("timeout".to_string(), s),
        };
        let signature_top3 = extract_signature_top3(&merged_output);

        let log_path = triage_dir.join(format!("attempt-{}.log", attempt));
        let log_body = format!(
            "attempt: {}\nresult: {}\nsignature_top3: {:?}\n{}\n",
            attempt, result_label, signature_top3, merged_output
        );
        fs::write(&log_path, log_body)
            .map_err(|e| format!("failed to write '{}': {e}", log_path.display()))?;

        attempts.push(TriageAttempt {
            attempt,
            result: result_label,
            signature_top3,
        });
    }

    let timeout_count = attempts.iter().filter(|a| a.result == "timeout").count();
    let success_count = attempts.iter().filter(|a| a.result == "success").count();
    let failed_count = attempts.iter().filter(|a| a.result == "failed").count();

    let mut signature_consistent = true;
    if let Some(first) = attempts.first().map(|a| &a.signature_top3) {
        signature_consistent = attempts.iter().all(|a| &a.signature_top3 == first);
    }

    let verdict = if timeout_count > 0 {
        "timeout"
    } else if success_count == attempts.len() && signature_consistent {
        "reproduced"
    } else if success_count <= 1 {
        "flaky"
    } else if !signature_consistent {
        "flaky_stack_mismatch"
    } else {
        "failed"
    };

    let summary_path = triage_dir.join("summary.json");
    let attempts_json = attempts
        .iter()
        .map(|a| {
            let sig = a
                .signature_top3
                .iter()
                .map(|s| format!("\"{}\"", json_escape(s)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    {{\"attempt\": {}, \"result\": \"{}\", \"signature_top3\": [{}]}}",
                a.attempt, a.result, sig
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let summary = format!(
        "{{\n  \"triage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"input\": \"{}\",\n  \"repro_retries\": {},\n  \"timeout_sec\": {},\n  \"success_count\": {},\n  \"failed_count\": {},\n  \"timeout_count\": {},\n  \"signature_consistent\": {},\n  \"verdict\": \"{}\",\n  \"attempts\": [\n{}\n  ]\n}}\n",
        triage_id,
        target_label(&args.target),
        json_escape(&args.input.display().to_string()),
        args.repro_retries,
        args.timeout_sec,
        success_count,
        failed_count,
        timeout_count,
        if signature_consistent { "true" } else { "false" },
        verdict,
        attempts_json
    );
    fs::write(&summary_path, summary)
        .map_err(|e| format!("failed to write '{}': {e}", summary_path.display()))?;

    println!("[triage] done");
    println!("target: {}", target_label(&args.target));
    println!("input: {}", args.input.display());
    println!("success_count: {success_count}");
    println!("failed_count: {failed_count}");
    println!("timeout_count: {timeout_count}");
    println!("signature_consistent: {signature_consistent}");
    println!("verdict: {verdict}");
    println!("summary: {}", summary_path.display());

    let valid_crashes = if verdict == "reproduced" { 1 } else { 0 };
    let new_crashes = if success_count > 0 { 1 } else { 0 };
    record_metrics_event(
        app_paths,
        MetricEvent {
            ts: now_unix(),
            kind: "triage",
            total: attempts.len() as u64,
            errors: (failed_count + timeout_count) as u64,
            new_paths: 0,
            new_crashes: new_crashes as u64,
            valid_crashes: valid_crashes as u64,
            total_crashes: new_crashes as u64,
        },
    )?;

    Ok(())
}

fn execute_triage_subprocess(
    target: &TargetKind,
    input: &Path,
    timeout_sec: u64,
    timeout_available: bool,
) -> Result<HarnessExecResult, String> {
    let exe = std::env::current_exe().map_err(|e| format!("failed to resolve current exe: {e}"))?;
    let mut cmd = if timeout_available {
        let mut c = command_with_core_dump_off("timeout");
        c.arg(format!("{}s", timeout_sec));
        c.arg(&exe);
        c
    } else {
        command_with_core_dump_off(&exe.display().to_string())
    };

    cmd.arg("harness")
        .arg("--target")
        .arg(target_label(target))
        .arg("--input")
        .arg(input.display().to_string())
        .env("OMP_NUM_THREADS", "1")
        .env("MKL_NUM_THREADS", "1")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("NUMEXPR_NUM_THREADS", "1")
        .env("VECLIB_MAXIMUM_THREADS", "1");

    let out = cmd
        .output()
        .map_err(|e| format!("failed to execute triage subprocess: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let merged = format!("{}\n{}", stdout, stderr);

    if timeout_available && out.status.code() == Some(124) {
        return Ok(HarnessExecResult::Timeout(merged));
    }
    if out.status.success() {
        return Ok(HarnessExecResult::Success(merged));
    }
    // OOM 137 triage 분기(DoS vs 인프라): v1은 infra_oom 힌트를 붙여 후속 triage/report에서 구분 가능하게 남긴다.
    if out.status.code() == Some(137) {
        return Ok(HarnessExecResult::Failed(format!("infra_oom:exit_137\n{}", merged)));
    }
    Ok(HarnessExecResult::Failed(merged))
}

fn extract_signature_top3(output: &str) -> Vec<String> {
    let mut selected = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if contains_stack_hint(trimmed) {
            selected.push(trimmed.to_string());
        }
        if selected.len() == 3 {
            return selected;
        }
    }

    // fallback: grab first 3 non-empty lines for stable comparison
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        selected.push(trimmed.to_string());
        if selected.len() == 3 {
            break;
        }
    }
    selected
}

fn contains_stack_hint(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("stack")
        || lower.contains("frame")
        || lower.contains("backtrace")
        || lower.contains("addresssanitizer")
        || lower.contains("segv")
        || lower.contains("sigabrt")
        || lower.contains("onnxruntimeerror")
        || lower.contains("load_fail")
}

fn run_harness(args: &HarnessArgs) -> Result<(), String> {
    if !args.input.exists() {
        return Err(format!("input not found: {}", args.input.display()));
    }
    if !args.input.is_file() {
        return Err(format!("input is not a file: {}", args.input.display()));
    }

    let bytes = fs::read(&args.input)
        .map_err(|e| format!("failed to read input '{}': {e}", args.input.display()))?;

    let parser_step = match args.target {
        TargetKind::Gguf => gguf_precheck(&bytes)?,
        TargetKind::Onnx => onnx_precheck(&bytes)?,
        TargetKind::Safetensors => safetensors_precheck(&bytes)?,
    };

    let core_path_step = match args.target {
        TargetKind::Gguf => "header->llama.cpp parser".to_string(),
        TargetKind::Onnx => "protobuf->onnxruntime session loader".to_string(),
        TargetKind::Safetensors => "header_json->safetensors safe_open".to_string(),
    };
    let library_connect = match args.target {
        TargetKind::Gguf => gguf_library_connect(&args.input),
        TargetKind::Onnx => onnx_library_connect(&args.input),
        TargetKind::Safetensors => safetensors_library_connect(&args.input),
    };
    let strict_connect = std::env::var("TOOL_REQUIRE_LIBRARY_CONNECT")
        .map(|v| v == "1")
        .unwrap_or(false);
    if strict_connect && !library_connect.connected {
        return Err(format!(
            "library connect required but unavailable: {}",
            library_connect.step
        ));
    }

    let external_step = maybe_run_external_harness(&args.target, &args.input)?;
    let report = HarnessReport {
        target: target_label(&args.target),
        input: args.input.display().to_string(),
        parser_step,
        core_path_step,
        library_step: library_connect.step,
        external_step,
    };
    print_harness_report(&report);
    Ok(())
}

fn target_label(target: &TargetKind) -> &'static str {
    match target {
        TargetKind::Gguf => "gguf",
        TargetKind::Onnx => "onnx",
        TargetKind::Safetensors => "safetensors",
    }
}

fn run_backend_label(backend: &RunBackend) -> &'static str {
    match backend {
        RunBackend::LocalHarness => "local-harness",
        RunBackend::Aflpp => "aflpp",
        RunBackend::Libfuzzer => "libfuzzer",
    }
}

fn seed_ext(target: &TargetKind) -> &'static str {
    match target {
        TargetKind::Gguf => "gguf",
        TargetKind::Onnx => "onnx",
        TargetKind::Safetensors => "safetensors",
    }
}

fn run_seed_tool(app_paths: &AppPaths, args: &SeedArgs) -> Result<(), String> {
    match &args.command {
        SeedCommands::Sync(sync) => run_seed_sync(app_paths, sync),
        SeedCommands::Stats(stats) => run_seed_stats(app_paths, stats),
    }
}

fn run_dashboard_snapshot(app_paths: &AppPaths, args: &DashboardArgs) -> Result<(), String> {
    let snap = collect_dashboard_snapshot(app_paths)?;
    match args.format {
        DashboardFormat::Json => {
            println!("{}", ui::dashboard::render_dashboard_json(&snap));
            Ok(())
        }
        DashboardFormat::Html => {
            let Some(out) = &args.out else {
                return Err("html format requires --out <path>".to_string());
            };
            let html = ui::dashboard::render_dashboard_html(&snap);
            if let Some(parent) = out.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent).map_err(|e| {
                        format!("failed to create dashboard dir '{}': {e}", parent.display())
                    })?;
                }
            }
            fs::write(out, html)
                .map_err(|e| format!("failed to write dashboard html '{}': {e}", out.display()))?;
            println!("[dashboard] done");
            println!("format: html");
            println!("out: {}", out.display());
            Ok(())
        }
    }
}

pub(crate) fn collect_dashboard_snapshot(app_paths: &AppPaths) -> Result<DashboardSnapshot, String> {
    let runs_root = app_paths.data_dir.join("runs");
    let triage_root = app_paths.data_dir.join("triage");
    let reports_root = app_paths.data_dir.join("reports");
    let coverage_root = app_paths.data_dir.join("coverage");
    let metrics_path = app_paths.data_dir.join("metrics").join("latest.json");
    let seeds_onnx_count = count_seed_files(&app_paths.seeds_dir.join("onnx"), "onnx")?;
    let seeds_gguf_count = count_seed_files(&app_paths.seeds_dir.join("gguf"), "gguf")?;
    let seeds_safetensors_count = count_seed_files(&app_paths.seeds_dir.join("safetensors"), "safetensors")?;
    let seeds_total_count = seeds_onnx_count + seeds_gguf_count + seeds_safetensors_count;

    let runs_count = count_prefixed_dirs(&runs_root, "run-")?;
    let triage_count = count_prefixed_dirs(&triage_root, "triage-")?;
    let report_count = count_prefixed_dirs(&reports_root, "report-")?;
    let coverage_count = count_prefixed_dirs(&coverage_root, "coverage-")?;

    let latest_run = latest_prefixed_dir_name(&runs_root, "run-")?.unwrap_or_else(|| "none".to_string());
    let latest_triage =
        latest_prefixed_dir_name(&triage_root, "triage-")?.unwrap_or_else(|| "none".to_string());
    let latest_report =
        latest_prefixed_dir_name(&reports_root, "report-")?.unwrap_or_else(|| "none".to_string());
    let latest_coverage =
        latest_prefixed_dir_name(&coverage_root, "coverage-")?.unwrap_or_else(|| "none".to_string());
    let recent_triage_ids = recent_prefixed_dir_names(&triage_root, "triage-", 8)?;
    let recent_report_ids = recent_prefixed_dir_names(&reports_root, "report-", 8)?;
    let recent_coverage_ids = recent_prefixed_dir_names(&coverage_root, "coverage-", 8)?;
    let latest_coverage_summary = if latest_coverage == "none" {
        "none".to_string()
    } else {
        coverage_root
            .join(&latest_coverage)
            .join("summary.json")
            .display()
            .to_string()
    };

    let mut new_paths_per_hour = "0".to_string();
    let mut new_crashes_per_hour = "0".to_string();
    let mut valid_crash_ratio = "0.0".to_string();
    let mut global_error_rate_5m = "0.0".to_string();
    let metrics_exists = metrics_path.exists();
    if metrics_exists {
        let metrics = fs::read_to_string(&metrics_path)
            .map_err(|e| format!("failed to read '{}': {e}", metrics_path.display()))?;
        new_paths_per_hour =
            extract_json_number_literal(&metrics, "new_paths_per_hour").unwrap_or_else(|| "0".to_string());
        new_crashes_per_hour =
            extract_json_number_literal(&metrics, "new_crashes_per_hour").unwrap_or_else(|| "0".to_string());
        valid_crash_ratio =
            extract_json_number_literal(&metrics, "valid_crash_ratio").unwrap_or_else(|| "0.0".to_string());
        global_error_rate_5m =
            extract_json_number_literal(&metrics, "global_error_rate_5m").unwrap_or_else(|| "0.0".to_string());
    }

    let latest_valid = find_latest_reproduced_triage(&triage_root)?;
    let (latest_valid_triage, latest_valid_input, latest_valid_signature, latest_valid_summary, latest_valid_report) =
        if let Some(item) = latest_valid {
            let report =
                find_report_by_source_triage_id(&reports_root, &item.triage_id)?.unwrap_or_else(|| "none".to_string());
            (
                format!("triage-{}", item.triage_id),
                item.input,
                item.signature_top1,
                item.summary_path,
                report,
            )
        } else {
            (
                "none".to_string(),
                "none".to_string(),
                "none".to_string(),
                "none".to_string(),
                "none".to_string(),
            )
        };

    Ok(DashboardSnapshot {
        generated_at: now_unix(),
        data_dir: app_paths.data_dir.display().to_string(),
        seeds_dir: app_paths.seeds_dir.display().to_string(),
        runs_count,
        triage_count,
        report_count,
        latest_run,
        latest_triage,
        latest_report,
        metrics_exists,
        new_paths_per_hour,
        new_crashes_per_hour,
        valid_crash_ratio,
        global_error_rate_5m,
        latest_valid_triage,
        latest_valid_input,
        latest_valid_signature,
        latest_valid_summary,
        latest_valid_report,
        recent_triage_ids,
        recent_report_ids,
        recent_coverage_ids,
        seeds_onnx_count,
        seeds_gguf_count,
        seeds_safetensors_count,
        seeds_total_count,
        coverage_count,
        latest_coverage,
        latest_coverage_summary,
    })
}

fn count_seed_files(root: &Path, ext: &str) -> Result<usize, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if path.is_file() && has_ext(&path, ext) {
            count += 1;
        }
    }
    Ok(count)
}

fn run_coverage_job(app_paths: &AppPaths, args: &CoverageArgs) -> Result<(), String> {
    let corpus_dir = args
        .corpus_dir
        .clone()
        .unwrap_or_else(|| app_paths.seeds_dir.join(target_label(&args.target)));
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!("corpus_dir is invalid: {}", corpus_dir.display()));
    }

    let mut inputs = collect_corpus_inputs(&corpus_dir, &args.target)?;
    if inputs.is_empty() {
        return Err(format!(
            "no input files found for target '{}' in {}",
            target_label(&args.target),
            corpus_dir.display()
        ));
    }
    if let Some(max_jobs) = args.max_jobs {
        inputs.truncate(max_jobs);
    }

    let coverage_id = now_unix_millis();
    let coverage_dir = app_paths
        .data_dir
        .join("coverage")
        .join(format!("coverage-{coverage_id}"));
    let logs_dir = coverage_dir.join("logs");
    fs::create_dir_all(&logs_dir)
        .map_err(|e| format!("failed to create coverage dir '{}': {e}", coverage_dir.display()))?;

    println!("[coverage] start");
    println!("target: {}", target_label(&args.target));
    println!("corpus_dir: {}", corpus_dir.display());
    println!("timeout_sec: {}", args.timeout_sec);
    println!("coverage_dir: {}", coverage_dir.display());

    let timeout_available = command_exists("timeout");
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut timeout = 0usize;

    for (i, input) in inputs.iter().enumerate() {
        let job = RunJob {
            id: i,
            input: input.clone(),
        };
        let result = execute_harness_subprocess(&job, &args.target, args.timeout_sec, timeout_available)?;
        write_job_log(&logs_dir, &job, 1, &result)?;
        match result {
            HarnessExecResult::Success(_) => success += 1,
            HarnessExecResult::Failed(_) => failed += 1,
            HarnessExecResult::Timeout(_) => timeout += 1,
        }
    }

    let total = success + failed + timeout;
    let summary_path = coverage_dir.join("summary.json");
    let summary = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"coverage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"corpus_dir\": \"{}\",\n  \"timeout_sec\": {},\n  \"total\": {},\n  \"success\": {},\n  \"failed\": {},\n  \"timeout\": {},\n  \"coverage_proxy\": {{\n    \"success_ratio\": {:.4}\n  }},\n  \"generated_at\": {}\n}}\n",
        coverage_id,
        target_label(&args.target),
        json_escape(&corpus_dir.display().to_string()),
        args.timeout_sec,
        total,
        success,
        failed,
        timeout,
        if total == 0 {
            0.0
        } else {
            success as f64 / total as f64
        },
        now_unix()
    );
    fs::write(&summary_path, summary)
        .map_err(|e| format!("failed to write '{}': {e}", summary_path.display()))?;

    println!("[coverage] done");
    println!("total: {total}");
    println!("success: {success}");
    println!("failed: {failed}");
    println!("timeout: {timeout}");
    println!("summary: {}", summary_path.display());
    Ok(())
}

struct ReproducedTriageView {
    triage_id: String,
    input: String,
    signature_top1: String,
    summary_path: String,
}

fn find_latest_reproduced_triage(triage_root: &Path) -> Result<Option<ReproducedTriageView>, String> {
    if !triage_root.exists() {
        return Ok(None);
    }

    let mut latest: Option<(u128, ReproducedTriageView)> = None;
    for entry in fs::read_dir(triage_root)
        .map_err(|e| format!("failed to read '{}': {e}", triage_root.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read triage entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id_text) = name.strip_prefix("triage-") else {
            continue;
        };
        let Ok(id) = id_text.parse::<u128>() else {
            continue;
        };

        let summary_path = path.join("summary.json");
        if !summary_path.exists() {
            continue;
        }
        let summary = fs::read_to_string(&summary_path)
            .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;
        let verdict = extract_json_string_literal(&summary, "verdict").unwrap_or_default();
        if verdict != "reproduced" {
            continue;
        }
        let input = extract_json_string_literal(&summary, "input").unwrap_or_else(|| "unknown".to_string());
        let signature_top1 =
            extract_first_signature_top3(&summary).unwrap_or_else(|| "none".to_string());

        let item = ReproducedTriageView {
            triage_id: id_text.to_string(),
            input,
            signature_top1,
            summary_path: summary_path.display().to_string(),
        };
        match &latest {
            Some((best, _)) if id <= *best => {}
            _ => latest = Some((id, item)),
        }
    }
    Ok(latest.map(|(_, item)| item))
}

fn find_report_by_source_triage_id(reports_root: &Path, triage_id: &str) -> Result<Option<String>, String> {
    if !reports_root.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(reports_root)
        .map_err(|e| format!("failed to read '{}': {e}", reports_root.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read report entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        let meta = fs::read_to_string(&meta_path)
            .map_err(|e| format!("failed to read '{}': {e}", meta_path.display()))?;
        let source_triage = extract_json_string_literal(&meta, "source_triage_id").unwrap_or_default();
        if source_triage != triage_id {
            continue;
        }
        let report_path = path.join("report.md");
        if report_path.exists() {
            return Ok(Some(report_path.display().to_string()));
        }
        return Ok(Some(path.display().to_string()));
    }
    Ok(None)
}

fn count_prefixed_dirs(root: &Path, prefix: &str) -> Result<usize, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(prefix) {
            count += 1;
        }
    }
    Ok(count)
}

fn latest_prefixed_dir_name(root: &Path, prefix: &str) -> Result<Option<String>, String> {
    if !root.exists() {
        return Ok(None);
    }
    let mut latest: Option<(u128, String)> = None;
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id_text) = name.strip_prefix(prefix) else {
            continue;
        };
        let Ok(id) = id_text.parse::<u128>() else {
            continue;
        };
        match &latest {
            Some((best, _)) if id <= *best => {}
            _ => latest = Some((id, name.to_string())),
        }
    }
    Ok(latest.map(|(_, n)| n))
}

fn recent_prefixed_dir_names(root: &Path, prefix: &str, limit: usize) -> Result<Vec<String>, String> {
    if !root.exists() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut rows: Vec<(u128, String)> = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id_text) = name.strip_prefix(prefix) else {
            continue;
        };
        let Ok(id) = id_text.parse::<u128>() else {
            continue;
        };
        rows.push((id, name.to_string()));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(rows.into_iter().take(limit).map(|(_, name)| name).collect())
}

fn run_seed_sync(app_paths: &AppPaths, args: &SeedSyncArgs) -> Result<(), String> {
    if !args.from.exists() || !args.from.is_dir() {
        return Err(format!("source dir is invalid: {}", args.from.display()));
    }
    let ext = seed_ext(&args.target);
    let dest_dir = args
        .to
        .clone()
        .unwrap_or_else(|| app_paths.seeds_dir.join(target_label(&args.target)));
    fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("failed to create dest dir '{}': {e}", dest_dir.display()))?;

    let mut existing_hashes = HashSet::new();
    for entry in fs::read_dir(&dest_dir)
        .map_err(|e| format!("failed to read dest dir '{}': {e}", dest_dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read dest entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() || !has_ext(&path, ext) {
            continue;
        }
        if let Ok(h) = sha256_file(&path) {
            existing_hashes.insert(h);
        }
    }

    let mut scanned = 0usize;
    let mut matched_ext = 0usize;
    let mut copied = 0usize;
    let mut dup_skipped = 0usize;
    let mut invalid_skipped = 0usize;
    let mut error_skipped = 0usize;

    for entry in fs::read_dir(&args.from)
        .map_err(|e| format!("failed to read source dir '{}': {e}", args.from.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read source entry: {e}"))?;
        let src = entry.path();
        if !src.is_file() {
            continue;
        }
        scanned += 1;
        if !has_ext(&src, ext) {
            continue;
        }
        matched_ext += 1;
        let hash = match sha256_file(&src) {
            Ok(h) => h,
            Err(_) => {
                error_skipped += 1;
                continue;
            }
        };
        if existing_hashes.contains(&hash) {
            dup_skipped += 1;
            continue;
        }
        if args.harness_filter {
            let valid = seed_harness_validate(&args.target, &src)?;
            if !valid {
                invalid_skipped += 1;
                continue;
            }
        }
        let dest = unique_dest_path(&dest_dir, &src, ext)?;
        fs::copy(&src, &dest).map_err(|e| {
            format!(
                "failed to copy '{}' -> '{}': {e}",
                src.display(),
                dest.display()
            )
        })?;
        existing_hashes.insert(hash);
        copied += 1;
    }

    println!("[seed sync] done");
    println!("target: {}", target_label(&args.target));
    println!("from: {}", args.from.display());
    println!("to: {}", dest_dir.display());
    println!("scanned: {scanned}");
    println!("matched_ext: {matched_ext}");
    println!("copied: {copied}");
    println!("dup_skipped: {dup_skipped}");
    println!("invalid_skipped: {invalid_skipped}");
    println!("error_skipped: {error_skipped}");
    Ok(())
}

fn run_seed_stats(app_paths: &AppPaths, args: &SeedStatsArgs) -> Result<(), String> {
    let ext = seed_ext(&args.target);
    let dir = args
        .dir
        .clone()
        .unwrap_or_else(|| app_paths.seeds_dir.join(target_label(&args.target)));
    if !dir.exists() || !dir.is_dir() {
        return Err(format!("seed dir is invalid: {}", dir.display()));
    }

    let mut total = 0usize;
    let mut unique = HashSet::new();
    let mut hash_errors = 0usize;
    let mut valid = 0usize;
    let mut invalid = 0usize;
    let mut validated = 0usize;
    for entry in fs::read_dir(&dir)
        .map_err(|e| format!("failed to read seed dir '{}': {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read seed entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() || !has_ext(&path, ext) {
            continue;
        }
        total += 1;
        match sha256_file(&path) {
            Ok(h) => {
                unique.insert(h);
            }
            Err(_) => hash_errors += 1,
        }
        match seed_harness_validate(&args.target, &path) {
            Ok(true) => {
                valid += 1;
                validated += 1;
            }
            Ok(false) => {
                invalid += 1;
                validated += 1;
            }
            Err(_) => {}
        }
    }
    let deduped = unique.len();
    let duplicates = total.saturating_sub(deduped);
    let valid_ratio = if validated == 0 {
        0.0
    } else {
        valid as f64 / validated as f64
    };

    println!("[seed stats] done");
    println!("target: {}", target_label(&args.target));
    println!("dir: {}", dir.display());
    println!("total: {total}");
    println!("unique: {deduped}");
    println!("duplicates: {duplicates}");
    println!("valid: {valid}");
    println!("invalid: {invalid}");
    println!("validated: {validated}");
    println!("valid_ratio: {:.4}", valid_ratio);
    println!("hash_errors: {hash_errors}");
    Ok(())
}

fn unique_dest_path(dest_dir: &Path, src: &Path, ext: &str) -> Result<PathBuf, String> {
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("seed")
        .to_string();
    let mut candidate = dest_dir.join(format!("{stem}.{ext}"));
    let mut n = 1usize;
    while candidate.exists() {
        candidate = dest_dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    Ok(candidate)
}

fn seed_harness_validate(target: &TargetKind, input: &Path) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| format!("failed to resolve current exe: {e}"))?;
    let out = command_with_core_dump_off(&exe.display().to_string())
        .arg("harness")
        .arg("--target")
        .arg(target_label(target))
        .arg("--input")
        .arg(input.display().to_string())
        .output()
        .map_err(|e| {
            format!(
                "failed to execute harness validation for '{}': {e}",
                input.display()
            )
        })?;
    Ok(out.status.success())
}

struct RunStatusCounts {
    total: usize,
    success: usize,
    failed: usize,
    timeout: usize,
    retries: usize,
}

fn write_run_status(
    run_dir: &Path,
    run_id: u128,
    target: &TargetKind,
    counts: RunStatusCounts,
    workers: usize,
    timeout_sec: u64,
    restart_limit: u32,
) -> Result<PathBuf, String> {
    let status_path = run_dir.join("status.json");
    let status_json = format!(
        "{{\n  \"run_id\": \"{}\",\n  \"target\": \"{}\",\n  \"total\": {},\n  \"success\": {},\n  \"failed\": {},\n  \"timeout\": {},\n  \"retries\": {},\n  \"workers\": {},\n  \"timeout_sec\": {},\n  \"restart_limit\": {}\n}}\n",
        run_id,
        target_label(target),
        counts.total,
        counts.success,
        counts.failed,
        counts.timeout,
        counts.retries,
        workers,
        timeout_sec,
        restart_limit
    );
    fs::write(&status_path, status_json)
        .map_err(|e| format!("failed to write '{}': {e}", status_path.display()))?;
    Ok(status_path)
}

fn gguf_precheck(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 24 {
        return Err("GGUF too small: need at least 24 bytes".to_string());
    }
    if &bytes[0..4] != b"GGUF" {
        return Err("GGUF magic mismatch".to_string());
    }

    let version = read_le_u32(&bytes[4..8])?;
    let tensor_count = read_le_u64(&bytes[8..16])?;
    let kv_count = read_le_u64(&bytes[16..24])?;
    Ok(format!(
        "GGUF ok (version={version}, kv_count={kv_count}, tensor_count={tensor_count})"
    ))
}

fn onnx_precheck(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("ONNX input is empty".to_string());
    }

    let (tag, tag_len) = decode_varint(bytes, 0)?;
    if tag != 0x08 {
        return Err(format!(
            "ONNX first field mismatch: expected tag 0x08(ir_version), got 0x{tag:x}"
        ));
    }

    let (ir_version, _) = decode_varint(bytes, tag_len)?;
    Ok(format!("ONNX protobuf probe ok (ir_version={ir_version})"))
}

fn safetensors_precheck(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 8 {
        return Err("safetensors too small: need at least 8 bytes".to_string());
    }

    let header_len = read_le_u64(&bytes[0..8])? as usize;
    if header_len == 0 {
        return Err("safetensors header length is zero".to_string());
    }
    let end = 8usize
        .checked_add(header_len)
        .ok_or_else(|| "safetensors header length overflow".to_string())?;
    if end > bytes.len() {
        return Err("safetensors header out of range".to_string());
    }

    let header = std::str::from_utf8(&bytes[8..end])
        .map_err(|e| format!("safetensors header utf8 error: {e}"))?;
    let trimmed = header.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return Err("safetensors header is not JSON-like object".to_string());
    }
    if !trimmed.contains(':') {
        return Err("safetensors header has no key/value entry".to_string());
    }

    Ok(format!(
        "safetensors header probe ok (header_bytes={header_len})"
    ))
}

fn read_le_u32(bytes: &[u8]) -> Result<u32, String> {
    if bytes.len() != 4 {
        return Err("read_le_u32 requires 4 bytes".to_string());
    }
    let mut arr = [0u8; 4];
    arr.copy_from_slice(bytes);
    Ok(u32::from_le_bytes(arr))
}

fn read_le_u64(bytes: &[u8]) -> Result<u64, String> {
    if bytes.len() != 8 {
        return Err("read_le_u64 requires 8 bytes".to_string());
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(bytes);
    Ok(u64::from_le_bytes(arr))
}

fn decode_varint(bytes: &[u8], offset: usize) -> Result<(u64, usize), String> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut idx = offset;

    while idx < bytes.len() {
        let b = bytes[idx];
        let low = (b & 0x7f) as u64;
        value |= low << shift;
        idx += 1;

        if (b & 0x80) == 0 {
            return Ok((value, idx - offset));
        }
        shift += 7;
        if shift >= 64 {
            return Err("protobuf varint too large".to_string());
        }
    }

    Err("protobuf varint truncated".to_string())
}

fn maybe_run_external_harness(target: &TargetKind, input: &Path) -> Result<String, String> {
    let env_key = match target {
        TargetKind::Gguf => "TOOL_GGUF_HARNESS_CMD",
        TargetKind::Onnx => "TOOL_ONNX_HARNESS_CMD",
        TargetKind::Safetensors => "TOOL_SAFETENSORS_HARNESS_CMD",
    };

    let Ok(command_line) = std::env::var(env_key) else {
        return Ok(format!("{env_key} not set (external harness skipped)"));
    };
    if command_line.trim().is_empty() {
        return Ok(format!("{env_key} empty (external harness skipped)"));
    }

    let mut parts = command_line.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return Ok(format!("{env_key} invalid (external harness skipped)"));
    }

    let cmd = parts.remove(0);
    let mut args = parts.into_iter().map(str::to_string).collect::<Vec<_>>();
    args.push(input.display().to_string());

    let status = command_with_core_dump_off(cmd)
        .args(&args)
        .status()
        .map_err(|e| format!("external harness command failed: {e}"))?;

    if status.success() {
        Ok(format!("{env_key} executed successfully"))
    } else {
        Ok(format!(
            "{env_key} executed but failed with status {}",
            status
        ))
    }
}

fn gguf_library_connect(input: &Path) -> LibraryConnectResult {
    let mut candidates = Vec::new();
    if let Ok(custom) = std::env::var("TOOL_LLAMA_CLI_BIN") {
        if !custom.trim().is_empty() {
            candidates.push(custom);
        }
    }
    candidates.push("llama-cli".to_string());
    candidates.push("tools/llama.cpp/build/bin/llama-cli".to_string());
    candidates.push("./tools/llama.cpp/build/bin/llama-cli".to_string());

    for cmd in candidates {
        let result = command_with_core_dump_off(&cmd)
            .args(["-m", &input.display().to_string(), "-n", "1", "-p", "hi"])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                return LibraryConnectResult {
                    step: format!("llama.cpp parser connected ({cmd} executed)"),
                    connected: true,
                };
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let lower = stderr.to_ascii_lowercase();
                if lower.contains("failed to execute") && lower.contains("no such file") {
                    continue;
                }
                return LibraryConnectResult {
                    step: format!(
                        "llama.cpp parser invoked (non-zero exit: {})",
                        first_line(&stderr)
                    ),
                    connected: true,
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return LibraryConnectResult {
                    step: format!("llama.cpp parser error ({e})"),
                    connected: false,
                }
            }
        }
    }

    LibraryConnectResult {
        step: "llama.cpp parser unavailable (llama-cli not installed)".to_string(),
        connected: false,
    }
}

fn onnx_library_connect(input: &Path) -> LibraryConnectResult {
    let code = r#"
import sys
try:
    import onnxruntime as ort
except Exception as e:
    print(f"missing_module:{e}")
    sys.exit(3)
path = sys.argv[1]
try:
    sess = ort.InferenceSession(path, providers=["CPUExecutionProvider"])
    print(f"session_ok:inputs={len(sess.get_inputs())},outputs={len(sess.get_outputs())}")
except Exception as e:
    print(f"load_fail:{e}")
    sys.exit(2)
"#;
    let python_bin = detect_python_bin();
    match command_with_core_dump_off(&python_bin)
        .args(["-c", code, &input.display().to_string()])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            LibraryConnectResult {
                step: format!("onnxruntime connected ({})", first_line(&stdout)),
                connected: true,
            }
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output.status.code() == Some(3) {
                LibraryConnectResult {
                    step: format!("onnxruntime unavailable ({})", first_line(&stdout)),
                    connected: false,
                }
            } else {
                LibraryConnectResult {
                    step: format!("onnxruntime loader invoked ({})", first_line(&stdout)),
                    connected: true,
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            LibraryConnectResult {
                step: format!("onnxruntime unavailable ({python_bin} not installed)"),
                connected: false,
            }
        }
        Err(e) => LibraryConnectResult {
            step: format!("onnxruntime probe error ({e})"),
            connected: false,
        },
    }
}

fn safetensors_library_connect(input: &Path) -> LibraryConnectResult {
    let code = r#"
import sys
try:
    from safetensors import safe_open
except Exception as e:
    print(f"missing_module:{e}")
    sys.exit(3)
path = sys.argv[1]
try:
    with safe_open(path, framework="pt", device="cpu") as f:
        keys = list(f.keys())
        print(f"safe_open_ok:tensors={len(keys)}")
except Exception as e:
    print(f"load_fail:{e}")
    sys.exit(2)
"#;
    let python_bin = detect_python_bin();
    match command_with_core_dump_off(&python_bin)
        .args(["-c", code, &input.display().to_string()])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            LibraryConnectResult {
                step: format!("safetensors connected ({})", first_line(&stdout)),
                connected: true,
            }
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output.status.code() == Some(3) {
                LibraryConnectResult {
                    step: format!("safetensors unavailable ({})", first_line(&stdout)),
                    connected: false,
                }
            } else {
                LibraryConnectResult {
                    step: format!("safetensors loader invoked ({})", first_line(&stdout)),
                    connected: true,
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            LibraryConnectResult {
                step: format!("safetensors unavailable ({python_bin} not installed)"),
                connected: false,
            }
        }
        Err(e) => LibraryConnectResult {
            step: format!("safetensors probe error ({e})"),
            connected: false,
        },
    }
}

fn detect_python_bin() -> String {
    if let Ok(custom) = std::env::var("TOOL_PYTHON_BIN") {
        if !custom.trim().is_empty() {
            return custom;
        }
    }

    let venv_python = Path::new(".venv/bin/python3");
    if venv_python.exists() {
        return venv_python.display().to_string();
    }

    "python3".to_string()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("no output").trim().to_string()
}

fn print_harness_report(report: &HarnessReport) {
    println!("[harness] done");
    println!("target: {}", report.target);
    println!("input: {}", report.input);
    println!("parser_step: {}", report.parser_step);
    println!("core_path_step: {}", report.core_path_step);
    println!("library_step: {}", report.library_step);
    println!("external_step: {}", report.external_step);
}

fn validate_official_source(source_url: &str, repo_prefix: &str) -> Result<(), String> {
    if !source_url.starts_with("https://") {
        return Err("source URL must use https".to_string());
    }

    let host = extract_host(source_url).ok_or_else(|| "source URL host is missing".to_string())?;
    if host != "github.com" && host != "codeload.github.com" {
        return Err(format!(
            "unsupported source host '{host}'; only github.com/codeload.github.com are allowed"
        ));
    }
    if !source_url.contains(repo_prefix) {
        return Err(format!(
            "source URL path must include official repository path '{repo_prefix}'"
        ));
    }

    Ok(())
}

fn extract_host(source_url: &str) -> Option<&str> {
    let without_scheme = source_url.strip_prefix("https://")?;
    let host = without_scheme.split('/').next()?;
    if host.is_empty() {
        return None;
    }
    Some(host)
}

fn download_file_name(source_url: &str) -> String {
    let without_query = source_url.split('?').next().unwrap_or(source_url);
    let from_path = without_query.rsplit('/').next().filter(|s| !s.is_empty());
    from_path.unwrap_or("target-source.bin").to_string()
}

fn download_file(source_url: &str, output_path: &Path) -> Result<(), String> {
    let output = output_path.display().to_string();

    if try_run(
        "curl",
        &[
            "-fL",
            "--retry",
            "2",
            "--connect-timeout",
            "15",
            "-o",
            &output,
            source_url,
        ],
    )? {
        return Ok(());
    }

    if try_run("wget", &["-O", &output, source_url])? {
        return Ok(());
    }

    Err("download failed: both curl and wget are unavailable or failed".to_string())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let file = path.display().to_string();

    if let Some(out) = run_capture("sha256sum", &[&file])? {
        return parse_hash_output(&out);
    }
    if let Some(out) = run_capture("shasum", &["-a", "256", &file])? {
        return parse_hash_output(&out);
    }

    Err("sha256 tool not found (sha256sum/shasum)".to_string())
}

fn parse_hash_output(output: &str) -> Result<String, String> {
    output
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or_else(|| "failed to parse sha256 output".to_string())
}

fn try_run(cmd: &str, args: &[&str]) -> Result<bool, String> {
    match Command::new(cmd).args(args).status() {
        Ok(status) => Ok(status.success()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{cmd} execution failed: {e}")),
    }
}

fn run_capture(cmd: &str, args: &[&str]) -> Result<Option<String>, String> {
    match Command::new(cmd).args(args).output() {
        Ok(output) => {
            if !output.status.success() {
                return Ok(None);
            }
            let text = String::from_utf8(output.stdout)
                .map_err(|e| format!("{cmd} output decode failed: {e}"))?;
            Ok(Some(text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{cmd} execution failed: {e}")),
    }
}

fn render_meta_json(meta: &TargetMeta) -> String {
    format!(
        "{{\n  \"schema_version\": \"{}\",\n  \"target\": \"{}\",\n  \"version\": \"{}\",\n  \"source_url\": \"{}\",\n  \"source_kind\": \"{}\",\n  \"downloaded_file\": \"{}\",\n  \"downloaded_sha256\": \"{}\",\n  \"downloaded_size_bytes\": {}\n}}\n",
        json_escape(meta.schema_version),
        json_escape(&meta.target),
        json_escape(&meta.version),
        json_escape(&meta.source_url),
        json_escape(meta.source_kind),
        json_escape(&meta.downloaded_file),
        json_escape(&meta.downloaded_sha256),
        meta.downloaded_size_bytes
    )
}

fn run_report_pipeline(app_paths: &AppPaths) -> Result<(), String> {
    report::run_report_pipeline(app_paths)
}

fn record_metrics_event(app_paths: &AppPaths, event: MetricEvent) -> Result<(), String> {
    metrics::record_metrics_event(app_paths, event)
}

fn extract_json_number_literal(json: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{}\":", key);
    let start = json.find(&key_pattern)? + key_pattern.len();
    let rest = &json[start..];

    let mut out = String::new();
    let mut started = false;
    for ch in rest.chars() {
        if !started {
            if ch.is_ascii_whitespace() {
                continue;
            }
            if ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.' {
                started = true;
                out.push(ch);
                continue;
            }
            return None;
        }

        if ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' || ch == '-' || ch == '+' {
            out.push(ch);
        } else {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn extract_json_string_literal(json: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{}\":", key);
    let start = json.find(&key_pattern)? + key_pattern.len();
    let rest = &json[start..];

    let mut value_start = None;
    for (idx, ch) in rest.char_indices() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch == '"' {
            value_start = Some(idx);
        }
        break;
    }
    let value_start = value_start?;
    let (value, _) = parse_json_string_literal_at(rest, value_start)?;
    Some(value)
}

fn extract_first_signature_top3(json: &str) -> Option<String> {
    let key_pattern = "\"signature_top3\":";
    let start = json.find(key_pattern)? + key_pattern.len();
    let rest = &json[start..];
    let array_start = rest.find('[')?;
    let array = &rest[array_start + 1..];

    let mut value_start = None;
    for (idx, ch) in array.char_indices() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch == '"' {
            value_start = Some(idx);
        }
        break;
    }
    let value_start = value_start?;
    let (value, _) = parse_json_string_literal_at(array, value_start)?;
    Some(value)
}

fn parse_json_string_literal_at(input: &str, quote_index: usize) -> Option<(String, usize)> {
    let mut chars = input[quote_index..].char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }

    let mut out = String::new();
    let mut escaped = false;
    for (offset, ch) in chars {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => out.push(ch),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let end = quote_index + offset + ch.len_utf8();
                return Some((out, end));
            }
            _ => out.push(ch),
        }
    }
    None
}

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn ensure_directory(path: &Path) -> std::io::Result<()> {
    if path.exists() && !path.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("path '{}' exists but is not a directory", path.display()),
        ));
    }
    fs::create_dir_all(path)
}

fn ensure_data_layout(data_dir: &Path) -> std::io::Result<()> {
    const REQUIRED_DIRS: &[&str] = &[
        "queue/pending",
        "queue/processing",
        "queue/done",
        "queue/failed",
        "queue/quarantine",
        "queue/quarantine/broken",
        "artifacts",
    ];

    for dir in REQUIRED_DIRS {
        fs::create_dir_all(data_dir.join(dir))?;
    }

    Ok(())
}

fn print_stub(command: &str, data_dir: &Path, seeds_dir: &Path) {
    println!("[{}] not implemented yet", command);
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}

fn print_stub_with_id(command: &str, data_dir: &Path, seeds_dir: &Path, id: &str) {
    println!("[{}] not implemented yet", command);
    println!("id: {}", id);
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}
