use std::{
    fs,
    path::{Path, PathBuf},
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
}

#[derive(Args)]
struct RunArgs {}

#[derive(Args)]
struct TriageArgs {
    /// Input file to reproduce
    #[arg(long)]
    input: PathBuf,
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
        Commands::Run(_args) => {
            print_stub("run", &app_paths.data_dir, &app_paths.seeds_dir);
        }
        Commands::Triage(args) => {
            print_stub_with_input("triage", &app_paths.data_dir, &app_paths.seeds_dir, &args.input);
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

fn print_stub_with_input(command: &str, data_dir: &Path, seeds_dir: &Path, input: &Path) {
    println!("[{}] not implemented yet", command);
    println!("input: {}", input.display());
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}

fn print_stub_with_id(command: &str, data_dir: &Path, seeds_dir: &Path, id: &str) {
    println!("[{}] not implemented yet", command);
    println!("id: {}", id);
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}
