use std::{
    collections::VecDeque,
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
    process::Command,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand};

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
struct ListArgs {}

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
            eprintln!("config error: {err}");
            return ExitCode::from(2);
        }
    };

    match cli.command {
        Commands::Run(args) => {
            if let Err(err) = run_fuzz_pipeline(&app_paths, &args) {
                eprintln!("run error: {err}");
                return ExitCode::from(5);
            }
        }
        Commands::Triage(args) => {
            if let Err(err) = run_triage_pipeline(&app_paths, &args) {
                eprintln!("triage error: {err}");
                return ExitCode::from(6);
            }
        }
        Commands::Report(_args) => {
            print_stub("report", &app_paths.data_dir, &app_paths.seeds_dir);
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
                eprintln!("prepare-target error: {err}");
                return ExitCode::from(3);
            }
        }
        Commands::Harness(args) => {
            if let Err(err) = run_harness(&args) {
                eprintln!("harness error: {err}");
                return ExitCode::from(4);
            }
        }
    }

    ExitCode::SUCCESS
}

struct AppPaths {
    data_dir: PathBuf,
    seeds_dir: PathBuf,
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
    direct_step: String,
    external_step: String,
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
    let corpus_dir = args
        .corpus_dir
        .clone()
        .unwrap_or_else(|| app_paths.seeds_dir.clone());
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!(
            "corpus_dir is invalid: {}",
            corpus_dir.display()
        ));
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

    let run_id = now_unix();
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

    let status_path = run_dir.join("status.json");
    let s = stats.lock().map_err(|_| "stats lock poisoned")?;
    let status_json = format!(
        "{{\n  \"run_id\": \"{}\",\n  \"target\": \"{}\",\n  \"total\": {},\n  \"success\": {},\n  \"failed\": {},\n  \"timeout\": {},\n  \"retries\": {},\n  \"workers\": {},\n  \"timeout_sec\": {},\n  \"restart_limit\": {}\n}}\n",
        run_id,
        target_label(&args.target),
        s.total,
        s.success,
        s.failed,
        s.timeout,
        s.retries,
        workers,
        args.timeout_sec,
        args.restart_limit
    );
    fs::write(&status_path, status_json)
        .map_err(|e| format!("failed to write '{}': {e}", status_path.display()))?;

    println!("[run] done");
    println!("success: {}", s.success);
    println!("failed: {}", s.failed);
    println!("timeout: {}", s.timeout);
    println!("retries: {}", s.retries);
    println!("status: {}", status_path.display());
    Ok(())
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
        let mut c = Command::new("timeout");
        c.arg(format!("{}s", timeout_sec));
        c.arg(&exe);
        c
    } else {
        Command::new(&exe)
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

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
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
        let mut c = Command::new("timeout");
        c.arg(format!("{}s", timeout_sec));
        c.arg(&exe);
        c
    } else {
        Command::new(&exe)
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

    // specs.md 13.1 요구사항의 "핵심 경로 호출" 자리를 고정: 추후 실제 라이브러리 호출로 교체.
    let core_path_step = match args.target {
        TargetKind::Gguf => "header->kv_count->tensor_count".to_string(),
        TargetKind::Onnx => "protobuf->graph_field_probe".to_string(),
        TargetKind::Safetensors => "header_json->tensor_offset_probe".to_string(),
    };

    let direct_step = match args.target {
        TargetKind::Gguf => gguf_direct_probe(&args.input),
        TargetKind::Onnx => onnx_direct_probe(&args.input),
        TargetKind::Safetensors => safetensors_direct_probe(&args.input),
    };

    let external_step = maybe_run_external_harness(&args.target, &args.input)?;
    let report = HarnessReport {
        target: target_label(&args.target),
        input: args.input.display().to_string(),
        parser_step,
        core_path_step,
        direct_step,
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

    let status = Command::new(cmd)
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

fn gguf_direct_probe(input: &Path) -> String {
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
        let result = Command::new(&cmd)
            .args(["-m", &input.display().to_string(), "-n", "1", "-p", "hi"])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                return format!("llama.cpp direct probe ok ({cmd} executed)");
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return format!("llama.cpp direct probe unavailable ({})", first_line(&stderr));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return format!("llama.cpp direct probe error ({e})"),
        }
    }

    "llama.cpp direct probe skipped (llama-cli not installed)".to_string()
}

fn onnx_direct_probe(input: &Path) -> String {
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
    match Command::new(&python_bin)
        .args(["-c", code, &input.display().to_string()])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            format!("onnxruntime direct probe ok ({})", first_line(&stdout))
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output.status.code() == Some(3) {
                format!("onnxruntime direct probe skipped ({})", first_line(&stdout))
            } else {
                format!("onnxruntime direct probe failed ({})", first_line(&stdout))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            format!("onnxruntime direct probe skipped ({python_bin} not installed)")
        }
        Err(e) => format!("onnxruntime direct probe error ({e})"),
    }
}

fn safetensors_direct_probe(input: &Path) -> String {
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
    match Command::new(&python_bin)
        .args(["-c", code, &input.display().to_string()])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            format!("safetensors direct probe ok ({})", first_line(&stdout))
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output.status.code() == Some(3) {
                format!("safetensors direct probe skipped ({})", first_line(&stdout))
            } else {
                format!("safetensors direct probe failed ({})", first_line(&stdout))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            format!("safetensors direct probe skipped ({python_bin} not installed)")
        }
        Err(e) => format!("safetensors direct probe error ({e})"),
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
    println!("direct_step: {}", report.direct_step);
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
