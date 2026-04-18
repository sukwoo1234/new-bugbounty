use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::common::{
    AppPaths, command_with_core_dump_off, download_file, download_file_name,
    first_line, has_ext, sha256_file, validate_official_source,
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
    pub(crate) external_step: String,
}

pub(crate) struct LibraryConnectResult {
    pub(crate) step: String,
    pub(crate) connected: bool,
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
            default_url: "https://github.com/microsoft/onnxruntime/archive/refs/tags/v1.23.2.tar.gz",
            official_owner: "microsoft",
            official_repo: "onnxruntime",
        },
        TargetKind::Safetensors => TargetPreset {
            name: "safetensors",
            default_version: "v0.7.0",
            default_url: "https://github.com/huggingface/safetensors/archive/refs/tags/v0.7.0.tar.gz",
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

pub(crate) fn collect_corpus_inputs(corpus_dir: &Path, target: &TargetKind) -> Result<Vec<PathBuf>, String> {
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
    let version = version_override
        .unwrap_or_else(|| preset.default_version.to_string());
    let source_url = source_url_override
        .unwrap_or_else(|| preset.default_url.to_string());

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

pub(crate) fn run_harness(target: &TargetKind, input: &Path) -> Result<(), String> {
    if !input.exists() {
        return Err(format!("input not found: {}", input.display()));
    }
    if !input.is_file() {
        return Err(format!("input is not a file: {}", input.display()));
    }

    let bytes = fs::read(input)
        .map_err(|e| format!("failed to read input '{}': {e}", input.display()))?;

    let parser_step = match target {
        TargetKind::Gguf => gguf_precheck(&bytes)?,
        TargetKind::Onnx => onnx_precheck(&bytes)?,
        TargetKind::Safetensors => safetensors_precheck(&bytes)?,
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
    if strict_connect && !library_connect.connected {
        return Err(format!(
            "library connect required but unavailable: {}",
            library_connect.step
        ));
    }

    let external_step = maybe_run_external_harness(target, input)?;
    let report = HarnessReport {
        target: target_label(target),
        input: input.display().to_string(),
        parser_step,
        core_path_step,
        library_step: library_connect.step,
        external_step,
    };
    print_harness_report(&report);
    Ok(())
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
                    step: format!("llama.cpp parser connected ({cmd}: {})", first_line(&stdout)),
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
