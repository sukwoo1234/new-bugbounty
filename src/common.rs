use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct AppPaths {
    pub(crate) data_dir: PathBuf,
    pub(crate) seeds_dir: PathBuf,
}

impl AppPaths {
    pub(crate) fn prepare(data_dir: &Path, seeds_dir: &Path) -> Result<Self, String> {
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

#[derive(Clone)]
pub(crate) struct ArtifactContract {
    pub(crate) runs_root: PathBuf,
    pub(crate) triage_root: PathBuf,
    pub(crate) reports_root: PathBuf,
    pub(crate) coverage_root: PathBuf,
    pub(crate) metrics_root: PathBuf,
}

pub(crate) fn artifact_contract(app_paths: &AppPaths) -> ArtifactContract {
    artifact_contract_for_data_dir(&app_paths.data_dir)
}

pub(crate) fn artifact_contract_for_data_dir(data_dir: &Path) -> ArtifactContract {
    ArtifactContract {
        runs_root: data_dir.join("runs"),
        triage_root: data_dir.join("triage"),
        reports_root: data_dir.join("reports"),
        coverage_root: data_dir.join("coverage"),
        metrics_root: data_dir.join("metrics"),
    }
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub(crate) fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

pub(crate) fn command_with_core_dump_off(program: &str) -> Command {
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

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
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

pub(crate) fn download_file(source_url: &str, output_path: &Path) -> Result<(), String> {
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

pub(crate) fn download_file_name(source_url: &str) -> String {
    let without_query = source_url.split('?').next().unwrap_or(source_url);
    let from_path = without_query.rsplit('/').next().filter(|s| !s.is_empty());
    from_path.unwrap_or("target-source.bin").to_string()
}

pub(crate) fn validate_official_source(
    source_url: &str,
    official_owner: &str,
    official_repo: &str,
) -> Result<(), String> {
    if !source_url.starts_with("https://") {
        return Err("source URL must use https".to_string());
    }

    let (host, segments) = parse_url_host_and_path(source_url)
        .ok_or_else(|| format!("source URL parse failed: '{source_url}'"))?;
    if host != "github.com" && host != "codeload.github.com" {
        return Err(format!(
            "unsupported source host '{host}'; only github.com/codeload.github.com are allowed"
        ));
    }
    // Path segment 정확 일치 검사 — query/fragment에 prefix 문자열을 끼워 넣는 우회 방지
    if segments.len() < 2 || segments[0] != official_owner || segments[1] != official_repo {
        return Err(format!(
            "source URL path must begin with '/{official_owner}/{official_repo}/'"
        ));
    }

    Ok(())
}

// `https://<host>/<seg1>/<seg2>/...?query#frag` → (host, [seg1, seg2, ...])
// query/fragment는 제거하고 비어있는 segment는 건너뛴다.
fn parse_url_host_and_path(source_url: &str) -> Option<(&str, Vec<&str>)> {
    let without_scheme = source_url.strip_prefix("https://")?;
    let without_fragment = without_scheme.split('#').next().unwrap_or(without_scheme);
    let without_query = without_fragment.split('?').next().unwrap_or(without_fragment);
    let mut parts = without_query.splitn(2, '/');
    let host = parts.next()?;
    if host.is_empty() {
        return None;
    }
    let path = parts.next().unwrap_or("");
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    Some((host, segments))
}

pub(crate) fn try_run(cmd: &str, args: &[&str]) -> Result<bool, String> {
    match Command::new(cmd).args(args).status() {
        Ok(status) => Ok(status.success()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{cmd} execution failed: {e}")),
    }
}

pub(crate) fn run_capture(cmd: &str, args: &[&str]) -> Result<Option<String>, String> {
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

pub(crate) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("no output").trim().to_string()
}

pub(crate) fn has_ext(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

pub(crate) fn shell_escape(path: &Path) -> String {
    let s = path.display().to_string();
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

pub(crate) fn ensure_directory(path: &Path) -> std::io::Result<()> {
    if path.exists() && !path.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("path '{}' exists but is not a directory", path.display()),
        ));
    }
    fs::create_dir_all(path)
}

pub(crate) fn ensure_data_layout(data_dir: &Path) -> std::io::Result<()> {
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

pub(crate) enum HarnessExecResult {
    Success(String),
    Failed(String),
    Timeout(String),
}
