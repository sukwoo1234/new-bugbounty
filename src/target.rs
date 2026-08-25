use std::{
    fs,
    path::{Path, PathBuf},
    process::{ExitStatus, Output},
};

use crate::common::{
    command_with_core_dump_off, download_file, download_file_name, first_line, has_ext,
    is_core_dump_wrapper_exec_failure, sha256_file, validate_official_source, AppPaths,
};
use crate::json_utils::json_escape;

#[derive(clap::ValueEnum, Clone, Debug)]
pub(crate) enum TargetKind {
    #[value(name = "gguf")]
    Gguf,
    #[value(name = "onnx")]
    Onnx,
    #[value(name = "safetensors")]
    Safetensors,
}

#[derive(Clone, Copy)]
pub(crate) struct TargetPreset {
    pub(crate) name: &'static str,
    pub(crate) default_version: &'static str,
    pub(crate) default_url: &'static str,
    pub(crate) official_owner: &'static str,
    pub(crate) official_repo: &'static str,
}

pub(crate) struct TargetMeta {
    pub(crate) schema_version: &'static str,
    pub(crate) target: String,
    pub(crate) version: String,
    pub(crate) source_url: String,
    pub(crate) source_kind: &'static str,
    pub(crate) downloaded_file: String,
    pub(crate) downloaded_sha256: String,
    pub(crate) downloaded_size_bytes: u64,
}

pub(crate) struct HarnessReport {
    pub(crate) target: &'static str,
    pub(crate) input: String,
    pub(crate) parser_step: String,
    pub(crate) core_path_step: String,
    pub(crate) library_step: String,
    pub(crate) library_outcome: &'static str,
    pub(crate) external_step: String,
}

pub(crate) struct LibraryConnectResult {
    pub(crate) step: String,
    pub(crate) outcome: LibraryConnectOutcome,
}

#[derive(Clone, Copy)]
pub(crate) enum LibraryConnectOutcome {
    SessionOk,
    Invoked,
    Crashed,
    Unavailable,
}

impl LibraryConnectOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LibraryConnectOutcome::SessionOk => "session_ok",
            LibraryConnectOutcome::Invoked => "invoked",
            LibraryConnectOutcome::Crashed => "crashed",
            LibraryConnectOutcome::Unavailable => "unavailable",
        }
    }
}

pub(crate) struct TargetAdapter {
    pub(crate) target_label: &'static str,
    pub(crate) seed_subdir: &'static str,
    pub(crate) input_ext: &'static str,
}

pub(crate) fn target_label(target: &TargetKind) -> &'static str {
    match target {
        TargetKind::Gguf => "gguf",
        TargetKind::Onnx => "onnx",
        TargetKind::Safetensors => "safetensors",
    }
}

pub(crate) fn seed_ext(target: &TargetKind) -> &'static str {
    match target {
        TargetKind::Gguf => "gguf",
        TargetKind::Onnx => "onnx",
        TargetKind::Safetensors => "safetensors",
    }
}

pub(crate) fn preset_for_target(target: &TargetKind) -> TargetPreset {
    match target {
        TargetKind::Gguf => TargetPreset {
            name: "llama.cpp",
            default_version: "b7921",
            default_url: "https://github.com/ggml-org/llama.cpp/archive/refs/tags/b7921.tar.gz",
            official_owner: "ggml-org",
            official_repo: "llama.cpp",
        },
        TargetKind::Onnx => TargetPreset {
            name: "onnxruntime",
            default_version: "v1.23.2",
            default_url:
                "https://github.com/microsoft/onnxruntime/archive/refs/tags/v1.23.2.tar.gz",
            official_owner: "microsoft",
            official_repo: "onnxruntime",
        },
        TargetKind::Safetensors => TargetPreset {
            name: "safetensors",
            default_version: "v0.7.0",
            default_url:
                "https://github.com/huggingface/safetensors/archive/refs/tags/v0.7.0.tar.gz",
            official_owner: "huggingface",
            official_repo: "safetensors",
        },
    }
}

pub(crate) fn resolve_target_adapter(target: &TargetKind) -> TargetAdapter {
    TargetAdapter {
        target_label: target_label(target),
        seed_subdir: target_label(target),
        input_ext: seed_ext(target),
    }
}

pub(crate) fn default_seed_dir(app_paths: &AppPaths, target: &TargetKind) -> PathBuf {
    let adapter = resolve_target_adapter(target);
    app_paths.seeds_dir.join(adapter.seed_subdir)
}

pub(crate) fn collect_corpus_inputs(
    corpus_dir: &Path,
    target: &TargetKind,
) -> Result<Vec<PathBuf>, String> {
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

pub(crate) fn prepare_target(
    app_paths: &AppPaths,
    target: &TargetKind,
    version_override: Option<String>,
    source_url_override: Option<String>,
) -> Result<(), String> {
    let preset = preset_for_target(target);
    let version = version_override.unwrap_or_else(|| preset.default_version.to_string());
    let source_url = source_url_override.unwrap_or_else(|| preset.default_url.to_string());

    validate_official_source(&source_url, preset.official_owner, preset.official_repo)?;

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

/// G3: why a harness invocation failed. Only a real library crash is the finding we
/// fuzz for; an input the harness rejected before the library ran, and a harness that
/// could not run at all, are not. Each exits with its own code so `run`/`triage` and
/// the engine drivers can tell a finding from a rejected input from a broken host.
#[derive(Debug)]
pub(crate) enum HarnessError {
    LibraryCrash(String),
    InputRejected(String),
    Unavailable(String),
}

impl HarnessError {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            HarnessError::LibraryCrash(_) => crate::EXIT_HARNESS_LIBRARY_CRASH,
            HarnessError::InputRejected(_) => crate::EXIT_HARNESS_INPUT_REJECTED,
            HarnessError::Unavailable(_) => crate::EXIT_HARNESS_UNAVAILABLE,
        }
    }
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HarnessError::LibraryCrash(msg)
            | HarnessError::InputRejected(msg)
            | HarnessError::Unavailable(msg) => write!(f, "{msg}"),
        }
    }
}

pub(crate) fn run_harness(target: &TargetKind, input: &Path) -> Result<(), HarnessError> {
    if !input.exists() {
        return Err(HarnessError::InputRejected(format!(
            "input not found: {}",
            input.display()
        )));
    }
    if !input.is_file() {
        return Err(HarnessError::InputRejected(format!(
            "input is not a file: {}",
            input.display()
        )));
    }

    let bytes = fs::read(input).map_err(|e| {
        HarnessError::InputRejected(format!("failed to read input '{}': {e}", input.display()))
    })?;

    let parser_step = match target {
        TargetKind::Gguf => gguf_precheck(&bytes).map_err(HarnessError::InputRejected)?,
        TargetKind::Onnx => onnx_precheck(&bytes).map_err(HarnessError::InputRejected)?,
        TargetKind::Safetensors => {
            safetensors_precheck(&bytes).map_err(HarnessError::InputRejected)?
        }
    };

    let core_path_step = match target {
        TargetKind::Gguf => "header->llama.cpp parser".to_string(),
        TargetKind::Onnx => "protobuf->onnxruntime session loader".to_string(),
        TargetKind::Safetensors => "header_json->safetensors safe_open".to_string(),
    };
    let library_connect = match target {
        TargetKind::Gguf => gguf_library_connect(input),
        TargetKind::Onnx => onnx_library_connect(input),
        TargetKind::Safetensors => safetensors_library_connect(input),
    };
    let strict_connect = std::env::var("TOOL_REQUIRE_LIBRARY_CONNECT")
        .map(|v| v == "1")
        .unwrap_or(false);
    if strict_connect && matches!(library_connect.outcome, LibraryConnectOutcome::Unavailable) {
        // Environment problem, not a finding: the library never ran.
        return Err(HarnessError::Unavailable(format!(
            "library connect required but unavailable: {}",
            library_connect.step
        )));
    }

    let library_crashed = matches!(library_connect.outcome, LibraryConnectOutcome::Crashed);
    let library_crash_step = library_connect.step.clone();
    let external_step = if library_crashed {
        "skipped because library_connect crashed".to_string()
    } else {
        maybe_run_external_harness(target, input)?
    };
    let report = HarnessReport {
        target: target_label(target),
        input: input.display().to_string(),
        parser_step,
        core_path_step,
        library_step: library_connect.step,
        library_outcome: library_connect.outcome.as_str(),
        external_step,
    };
    print_harness_report(&report);
    if library_crashed {
        return Err(HarnessError::LibraryCrash(format!(
            "library connect crashed: {library_crash_step}"
        )));
    }
    Ok(())
}

fn print_harness_report(report: &HarnessReport) {
    println!("[harness] done");
    println!("target: {}", report.target);
    println!("input: {}", report.input);
    println!("parser_step: {}", report.parser_step);
    println!("core_path_step: {}", report.core_path_step);
    println!("library_step: {}", report.library_step);
    println!("library_outcome: {}", report.library_outcome);
    println!("external_step: {}", report.external_step);
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

/// Split a configured command line the way a shell would.
///
/// A33: `split_whitespace()` cut a quoted program path in half at its space, so a
/// correctly configured harness looked permanently unavailable - and when the
/// mangled prefix still resolved to something, its non-zero exit was filed as a
/// library crash. Handles single quotes (literal), double quotes (`\"` and `\\`
/// escapes) and a bare backslash; an unbalanced quote is an error rather than a
/// silently different command.
fn split_command_line(line: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    parts.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => current.push(c),
                        None => return Err("unbalanced single quote".to_string()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '\\')) => current.push(escaped),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return Err("unbalanced double quote".to_string()),
                        },
                        Some(c) => current.push(c),
                        None => return Err("unbalanced double quote".to_string()),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(escaped) => {
                    current.push(escaped);
                    started = true;
                }
                None => return Err("trailing backslash".to_string()),
            },
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        parts.push(current);
    }
    Ok(parts)
}

fn maybe_run_external_harness(target: &TargetKind, input: &Path) -> Result<String, HarnessError> {
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

    let mut parts = split_command_line(&command_line).map_err(|e| {
        HarnessError::Unavailable(format!("{env_key} is not a valid command line: {e}"))
    })?;
    if parts.is_empty() {
        return Ok(format!("{env_key} invalid (external harness skipped)"));
    }

    let cmd = parts.remove(0);
    let mut args = parts;
    args.push(input.display().to_string());

    let output = command_with_core_dump_off(&cmd)
        .args(&args)
        .output()
        .map_err(|e| HarnessError::Unavailable(format!("{env_key} could not be executed: {e}")))?;

    if output.status.success() {
        return Ok(format!("{env_key} executed successfully"));
    }

    // A13-class: the harness binary itself could not be executed (prlimit reports that as
    // a plain 126/127), which is a broken configuration rather than a library crash.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_core_dump_wrapper_exec_failure(output.status.code(), &stderr) {
        return Err(HarnessError::Unavailable(format!(
            "{env_key} could not be executed ({})",
            first_line(&stderr)
        )));
    }

    // Anything else stays a crash: sanitizers report findings through a plain non-zero
    // exit (ASAN exitcode=1), so downgrading here would lose crashes.
    Err(HarnessError::LibraryCrash(format!(
        "{env_key} executed but failed with status {} ({})",
        output.status,
        first_line(&stderr)
    )))
}

// --- Format-specific prechecks ---

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
    // ONNX는 protobuf 포맷이라 magic bytes가 없고, protobuf 스펙상 field 순서도
    // 보장되지 않는다 (specs.md §13.3: "protobuf 디코드 + 그래프/노드 순회").
    // 엄격한 첫 field 검증은 정상 모델을 거절할 위험이 있어 비운다.
    // 실제 구조 검증은 onnxruntime 라이브러리의 library_connect 단계에 위임.
    if bytes.is_empty() {
        return Err("ONNX input is empty".to_string());
    }
    Ok(format!("ONNX input accepted (size={} bytes)", bytes.len()))
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

// --- Library connect probes ---

fn gguf_library_connect(input: &Path) -> LibraryConnectResult {
    let mut candidates = Vec::new();
    if let Ok(custom) = std::env::var("TOOL_LLAMA_CLI_BIN") {
        if !custom.trim().is_empty() {
            let custom_path = PathBuf::from(custom);
            if let Some(bin_dir) = custom_path.parent() {
                candidates.push(bin_dir.join("llama-gguf-hash").display().to_string());
            }
        }
    }
    candidates.push("llama-gguf-hash".to_string());
    candidates.push("tools/llama.cpp/build/bin/llama-gguf-hash".to_string());
    candidates.push("./tools/llama.cpp/build/bin/llama-gguf-hash".to_string());

    for cmd in candidates {
        let result = command_with_core_dump_off(&cmd)
            .args(["--sha256", &input.display().to_string()])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                return LibraryConnectResult {
                    step: format!(
                        "llama.cpp parser connected ({cmd}: {})",
                        first_line(&stdout)
                    ),
                    outcome: LibraryConnectOutcome::SessionOk,
                };
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let lower = stderr.to_ascii_lowercase();
                // A13: prlimit reports its own exec failure (127/126) instead of
                // Err(NotFound); either way this candidate does not exist, so try the next.
                if is_core_dump_wrapper_exec_failure(output.status.code(), &stderr)
                    || (lower.contains("failed to execute") && lower.contains("no such file"))
                {
                    continue;
                }
                if let Some(result) =
                    crashed_connect_result("llama.cpp parser", "invocation", &output)
                {
                    return result;
                }
                return LibraryConnectResult {
                    step: format!(
                        "llama.cpp parser invoked (non-zero exit: {})",
                        first_line(&stderr)
                    ),
                    outcome: LibraryConnectOutcome::Invoked,
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return LibraryConnectResult {
                    step: format!("llama.cpp parser error ({e})"),
                    outcome: LibraryConnectOutcome::Unavailable,
                }
            }
        }
    }

    LibraryConnectResult {
        step: "llama.cpp parser unavailable (llama-cli not installed)".to_string(),
        outcome: LibraryConnectOutcome::Unavailable,
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
                outcome: LibraryConnectOutcome::SessionOk,
            }
        }
        Ok(output) => {
            if let Some(result) = crashed_connect_result("onnxruntime", "loader", &output) {
                return result;
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            // A13: the interpreter never ran (prlimit could not exec it), so this is an
            // unavailable probe, not an invoked loader.
            if is_core_dump_wrapper_exec_failure(output.status.code(), &stderr) {
                return LibraryConnectResult {
                    step: format!("onnxruntime unavailable ({})", first_line(&stderr)),
                    outcome: LibraryConnectOutcome::Unavailable,
                };
            }
            if output.status.code() == Some(3) {
                LibraryConnectResult {
                    step: format!("onnxruntime unavailable ({})", first_line(&stdout)),
                    outcome: LibraryConnectOutcome::Unavailable,
                }
            } else {
                LibraryConnectResult {
                    step: format!("onnxruntime loader invoked ({})", first_line(&stdout)),
                    outcome: LibraryConnectOutcome::Invoked,
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LibraryConnectResult {
            step: format!("onnxruntime unavailable ({python_bin} not installed)"),
            outcome: LibraryConnectOutcome::Unavailable,
        },
        Err(e) => LibraryConnectResult {
            step: format!("onnxruntime probe error ({e})"),
            outcome: LibraryConnectOutcome::Unavailable,
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
                outcome: LibraryConnectOutcome::SessionOk,
            }
        }
        Ok(output) => {
            if let Some(result) = crashed_connect_result("safetensors", "loader", &output) {
                return result;
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            // A13: see onnx_library_connect - a wrapper exec failure means no probe ran.
            if is_core_dump_wrapper_exec_failure(output.status.code(), &stderr) {
                return LibraryConnectResult {
                    step: format!("safetensors unavailable ({})", first_line(&stderr)),
                    outcome: LibraryConnectOutcome::Unavailable,
                };
            }
            if output.status.code() == Some(3) {
                LibraryConnectResult {
                    step: format!("safetensors unavailable ({})", first_line(&stdout)),
                    outcome: LibraryConnectOutcome::Unavailable,
                }
            } else {
                LibraryConnectResult {
                    step: format!("safetensors loader invoked ({})", first_line(&stdout)),
                    outcome: LibraryConnectOutcome::Invoked,
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LibraryConnectResult {
            step: format!("safetensors unavailable ({python_bin} not installed)"),
            outcome: LibraryConnectOutcome::Unavailable,
        },
        Err(e) => LibraryConnectResult {
            step: format!("safetensors probe error ({e})"),
            outcome: LibraryConnectOutcome::Unavailable,
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

fn crashed_connect_result(
    component: &str,
    action: &str,
    output: &Output,
) -> Option<LibraryConnectResult> {
    let detail = crash_status_detail(&output.status)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Some(LibraryConnectResult {
        step: format!(
            "{component} {action} crashed ({detail}; stdout: {}; stderr: {})",
            first_line(&stdout),
            first_line(&stderr)
        ),
        outcome: LibraryConnectOutcome::Crashed,
    })
}

fn crash_status_detail(status: &ExitStatus) -> Option<String> {
    if let Some(signal_number) = exit_signal(status) {
        return Some(format!(
            "{} (signal: {signal_number})",
            signal_name(signal_number)
        ));
    }

    let exit_code = status.code()?;
    exit_code_signal_name(exit_code).map(|signal| format!("{signal} (exit_code: {exit_code})"))
}

fn exit_code_signal_name(exit_code: i32) -> Option<&'static str> {
    match exit_code {
        132 => Some("SIGILL"),
        134 => Some("SIGABRT"),
        135 => Some("SIGBUS"),
        136 => Some("SIGFPE"),
        137 => Some("SIGKILL"),
        139 => Some("SIGSEGV"),
        143 => Some("SIGTERM"),
        _ => None,
    }
}

fn signal_name(signal_number: i32) -> &'static str {
    match signal_number {
        4 => "SIGILL",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        15 => "SIGTERM",
        _ => "unknown",
    }
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    // A33: the external harness command line was tokenized with split_whitespace(),
    // so a quoted program path containing a space was cut in half. The harness then
    // looked permanently unavailable, and when the mangled prefix happened to
    // resolve, its non-zero exit was filed as a library crash.
    #[test]
    fn harness_command_line_is_split_like_a_shell() {
        use super::split_command_line;

        assert_eq!(
            split_command_line("\"/tmp/my dir/replay\" --flag 'a b'").expect("split"),
            vec!["/tmp/my dir/replay", "--flag", "a b"]
        );
        assert_eq!(
            split_command_line("/usr/bin/env python3 x.py").expect("split"),
            vec!["/usr/bin/env", "python3", "x.py"]
        );
        assert_eq!(
            split_command_line("/tmp/my\\ dir/replay").expect("split"),
            vec!["/tmp/my dir/replay"]
        );
        assert_eq!(
            split_command_line("  spaced   out  ").expect("split"),
            vec!["spaced", "out"]
        );
        assert!(split_command_line("\"unterminated").is_err());
        assert!(split_command_line("'unterminated").is_err());
        assert!(split_command_line("").expect("split").is_empty());
    }

    use super::{crash_status_detail, exit_code_signal_name};

    // cargo runs #[test]s in parallel threads and TOOL_PYTHON_BIN /
    // TOOL_REQUIRE_LIBRARY_CONNECT are process-global, so the probe tests that
    // mutate them must not overlap.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn detects_128_plus_signal_exit_codes() {
        assert_eq!(exit_code_signal_name(136), Some("SIGFPE"));
        assert_eq!(exit_code_signal_name(139), Some("SIGSEGV"));
        assert_eq!(exit_code_signal_name(2), None);
    }

    #[cfg(unix)]
    #[test]
    fn detects_unix_signal_status() {
        use std::os::unix::process::ExitStatusExt;

        let status = std::process::ExitStatus::from_raw(11);
        assert_eq!(
            crash_status_detail(&status).as_deref(),
            Some("SIGSEGV (signal: 11)")
        );
    }

    // G1 regression: protect the crash-propagation fix (commit 0a0b475). A library
    // subprocess killed by a signal must be classified as a crash, and run_harness
    // must surface it as Err rather than a silent clean EXIT:0. Both tests are
    // self-contained: no private PoC and no real onnxruntime.
    #[cfg(unix)]
    #[test]
    fn signal_killed_library_output_is_classified_as_crashed() {
        use super::{crashed_connect_result, LibraryConnectOutcome};
        use std::os::unix::process::ExitStatusExt;

        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(11), // WIFSIGNALED, SIGSEGV
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        let result = crashed_connect_result("onnxruntime", "loader", &output)
            .expect("a signal-killed library subprocess must be classified as a crash");
        assert!(matches!(result.outcome, LibraryConnectOutcome::Crashed));
        assert!(result.step.contains("SIGSEGV"), "step was: {}", result.step);
    }

    #[cfg(unix)]
    #[test]
    fn run_harness_returns_err_when_library_subprocess_is_signal_killed() {
        use super::{run_harness, TargetKind};
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let _guard = env_lock();
        let dir = std::env::temp_dir().join(format!("tool-g1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Stand-in for the onnxruntime probe: dies by SIGSEGV instead of loading.
        let script = dir.join("segv_python.sh");
        {
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(f, "#!/bin/sh\nkill -SEGV \"$$\"").unwrap();
        }
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let input = dir.join("sample.onnx");
        std::fs::write(&input, b"not-a-real-onnx-but-non-empty").unwrap();

        std::env::set_var("TOOL_PYTHON_BIN", &script);
        std::env::remove_var("TOOL_ONNX_HARNESS_CMD");
        let result = run_harness(&TargetKind::Onnx, &input);
        std::env::remove_var("TOOL_PYTHON_BIN");
        let _ = std::fs::remove_dir_all(&dir);

        let err = result.expect_err(
            "a signal-killed library subprocess must make run_harness return Err, not Ok",
        );
        assert!(
            matches!(err, super::HarnessError::LibraryCrash(_)),
            "err was: {err:?}"
        );
        assert_eq!(err.exit_code(), crate::EXIT_HARNESS_LIBRARY_CRASH);
        assert!(err.to_string().contains("crash"), "err was: {err}");
    }

    // G3: exit code 4 must mean "the library really crashed". Benign harness-side
    // rejections (missing input, precheck reject) exit with a different code so
    // triage does not count them as crashes.
    #[test]
    fn missing_input_is_a_benign_harness_error() {
        use super::{run_harness, HarnessError, TargetKind};

        let missing = std::env::temp_dir().join(format!(
            "tool-g3-missing-{}-{}.onnx",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_file(&missing);

        let err = run_harness(&TargetKind::Onnx, &missing)
            .expect_err("a missing input must make run_harness return Err");
        assert!(
            matches!(err, HarnessError::InputRejected(_)),
            "err was: {err:?}"
        );
        assert_eq!(err.exit_code(), crate::EXIT_HARNESS_INPUT_REJECTED);
    }

    #[test]
    fn precheck_rejection_is_a_benign_harness_error() {
        use super::{run_harness, HarnessError, TargetKind};

        let dir = std::env::temp_dir().join(format!("tool-g3-precheck-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("tiny.safetensors");
        // shorter than the 8-byte header length prefix -> rejected before any library call
        std::fs::write(&input, b"abc").unwrap();

        let err = run_harness(&TargetKind::Safetensors, &input)
            .expect_err("a precheck-rejected input must make run_harness return Err");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            matches!(err, HarnessError::InputRejected(_)),
            "err was: {err:?}"
        );
        assert_eq!(err.exit_code(), crate::EXIT_HARNESS_INPUT_REJECTED);
    }

    // A13: with prlimit in front of the interpreter, a missing/non-executable
    // interpreter comes back as a plain exit 127 instead of Err(NotFound), so the
    // probe used to report "loader invoked" even though nothing ever ran.
    #[cfg(unix)]
    #[test]
    fn missing_python_interpreter_is_unavailable_not_invoked() {
        use super::{onnx_library_connect, safetensors_library_connect, LibraryConnectOutcome};

        let _guard = env_lock();
        let dir = std::env::temp_dir().join(format!("tool-a13-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("sample.onnx");
        std::fs::write(&input, b"bytes").unwrap();

        std::env::set_var("TOOL_PYTHON_BIN", "/nonexistent/tool-a13-python3");
        let onnx = onnx_library_connect(&input);
        let safetensors = safetensors_library_connect(&input);
        std::env::remove_var("TOOL_PYTHON_BIN");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            matches!(onnx.outcome, LibraryConnectOutcome::Unavailable),
            "onnx step was: {}",
            onnx.step
        );
        assert!(
            matches!(safetensors.outcome, LibraryConnectOutcome::Unavailable),
            "safetensors step was: {}",
            safetensors.step
        );
    }

    // Review follow-up: an external harness that cannot be executed is an environment
    // problem, not a crash; reporting it as a library crash files a finding for a typo.
    #[cfg(unix)]
    #[test]
    fn unrunnable_external_harness_is_unavailable_not_a_crash() {
        use super::{run_harness, HarnessError, TargetKind};

        let _guard = env_lock();
        let dir = std::env::temp_dir().join(format!("tool-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("sample.onnx");
        std::fs::write(&input, b"bytes").unwrap();

        std::env::set_var("TOOL_ONNX_HARNESS_CMD", "/nonexistent/tool-external-harness");
        std::env::remove_var("TOOL_REQUIRE_LIBRARY_CONNECT");
        let result = run_harness(&TargetKind::Onnx, &input);
        std::env::remove_var("TOOL_ONNX_HARNESS_CMD");
        let _ = std::fs::remove_dir_all(&dir);

        let err = result.expect_err("an unrunnable external harness must fail the harness run");
        assert!(
            matches!(err, HarnessError::Unavailable(_)),
            "err was: {err:?}"
        );
        assert_eq!(err.exit_code(), crate::EXIT_HARNESS_UNAVAILABLE);
    }

    #[cfg(unix)]
    #[test]
    fn strict_library_connect_gate_fails_when_interpreter_is_missing() {
        use super::{run_harness, HarnessError, TargetKind};

        let _guard = env_lock();
        let dir = std::env::temp_dir().join(format!("tool-a13-strict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("sample.onnx");
        std::fs::write(&input, b"bytes").unwrap();

        std::env::set_var("TOOL_PYTHON_BIN", "/nonexistent/tool-a13-python3");
        std::env::set_var("TOOL_REQUIRE_LIBRARY_CONNECT", "1");
        std::env::remove_var("TOOL_ONNX_HARNESS_CMD");
        let result = run_harness(&TargetKind::Onnx, &input);
        std::env::remove_var("TOOL_REQUIRE_LIBRARY_CONNECT");
        std::env::remove_var("TOOL_PYTHON_BIN");
        let _ = std::fs::remove_dir_all(&dir);

        let err = result.expect_err(
            "the strict library-connect gate must fail when the probe never ran",
        );
        assert!(
            matches!(err, HarnessError::Unavailable(_)),
            "err was: {err:?}"
        );
        assert_eq!(err.exit_code(), crate::EXIT_HARNESS_UNAVAILABLE);
        assert!(
            err.to_string().contains("unavailable"),
            "err was: {err}"
        );
    }
}
