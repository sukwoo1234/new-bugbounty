use std::{fs, path::Path, process::ExitStatus};

use crate::common::{
    command_exists, command_with_core_dump_off, now_unix, now_unix_millis, AppPaths,
};
use crate::json_utils::json_escape;
use crate::metrics::MetricEvent;
use crate::target::{target_label, TargetKind};

struct TriageAttempt {
    attempt: u32,
    result: String,
    exit_code: Option<i32>,
    signal: String,
    sanitizer: String,
    crash_kind: String,
    crash_summary: String,
    signature_top3: Vec<String>,
    top_frames: Vec<String>,
    normalized_frames: Vec<String>,
    normalized_frame_hash: String,
    timeout: bool,
    infra_oom: bool,
}

struct TriageExecResult {
    result: TriageResult,
    output: String,
    exit_code: Option<i32>,
    signal: Option<i32>,
}

enum TriageResult {
    Clean,
    Crashed,
    Timeout,
}

pub(crate) fn run_triage_pipeline(
    app_paths: &AppPaths,
    target: &TargetKind,
    input: &Path,
    repro_retries: u32,
    timeout_sec: u64,
) -> Result<(), String> {
    if !input.exists() || !input.is_file() {
        return Err(format!("input is invalid: {}", input.display()));
    }
    if repro_retries == 0 {
        return Err("repro_retries must be >= 1".to_string());
    }

    let triage_id = now_unix_millis();
    let triage_dir = app_paths
        .data_dir
        .join("triage")
        .join(format!("triage-{triage_id}"));
    fs::create_dir_all(&triage_dir).map_err(|e| {
        format!(
            "failed to create triage dir '{}': {e}",
            triage_dir.display()
        )
    })?;

    let timeout_available = command_exists("timeout");
    let mut attempts = Vec::new();

    for attempt in 1..=repro_retries {
        let exec = execute_triage_subprocess(target, input, timeout_sec, timeout_available)?;
        // harness exit 0 = 정상 종료 (clean), non-zero = 크래시 (crashed) per specs.md §3.1
        let result_label = match exec.result {
            TriageResult::Clean => "clean".to_string(),
            TriageResult::Crashed => "crashed".to_string(),
            TriageResult::Timeout => "timeout".to_string(),
        };
        let parsed = parse_crash_log(&exec.output, exec.exit_code, exec.signal, &result_label);

        let log_path = triage_dir.join(format!("attempt-{}.log", attempt));
        let log_body = format!(
            "attempt: {}\nresult: {}\nexit_code: {}\nsignal: {}\nsanitizer: {}\ncrash_kind: {}\nnormalized_frame_hash: {}\nsignature_top3: {:?}\ntop_frames: {:?}\nnormalized_frames: {:?}\n{}\n",
            attempt,
            result_label,
            option_i32_json_value(exec.exit_code),
            parsed.signal,
            parsed.sanitizer,
            parsed.crash_kind,
            parsed.normalized_frame_hash,
            parsed.signature_top3,
            parsed.top_frames,
            parsed.normalized_frames,
            exec.output
        );
        fs::write(&log_path, log_body)
            .map_err(|e| format!("failed to write '{}': {e}", log_path.display()))?;

        attempts.push(TriageAttempt {
            attempt,
            result: result_label,
            exit_code: exec.exit_code,
            signal: parsed.signal,
            sanitizer: parsed.sanitizer,
            crash_kind: parsed.crash_kind,
            crash_summary: parsed.crash_summary,
            signature_top3: parsed.signature_top3,
            top_frames: parsed.top_frames,
            normalized_frames: parsed.normalized_frames,
            normalized_frame_hash: parsed.normalized_frame_hash,
            timeout: parsed.timeout,
            infra_oom: parsed.infra_oom,
        });
    }

    let timeout_count = attempts.iter().filter(|a| a.result == "timeout").count();
    let clean_count = attempts.iter().filter(|a| a.result == "clean").count();
    let crashed_count = attempts.iter().filter(|a| a.result == "crashed").count();
    let infra_oom_count = attempts.iter().filter(|a| a.infra_oom).count();
    let manual_review_count = attempts
        .iter()
        .filter(|a| a.result == "crashed" && requires_manual_review(&a.crash_kind))
        .count();

    let mut signature_consistent = true;
    let comparable_hashes = attempts
        .iter()
        .filter(|a| a.result == "crashed" && !a.normalized_frame_hash.is_empty())
        .map(|a| a.normalized_frame_hash.as_str())
        .collect::<Vec<_>>();
    if let Some(first) = comparable_hashes.first() {
        signature_consistent = comparable_hashes.iter().all(|hash| hash == first);
    }

    // Explicit verdicts: confirmed reports only come from reproduced; ambiguous cases go to manual review.
    let verdict = if infra_oom_count > 0 {
        "infra_oom"
    } else if attempts.iter().any(|a| a.timeout) || timeout_count > 0 {
        "timeout"
    } else if crashed_count == 0 {
        "not_reproduced"
    } else if crashed_count == attempts.len() && signature_consistent && manual_review_count == 0 {
        "reproduced"
    } else if crashed_count == attempts.len() && manual_review_count > 0 {
        "manual_review"
    } else if crashed_count <= 1 || !signature_consistent {
        "flaky"
    } else {
        "manual_review"
    };

    let summary_path = triage_dir.join("summary.json");
    let representative = attempts
        .iter()
        .find(|a| a.result == "crashed")
        .or_else(|| attempts.first());
    let attempts_json = attempts
        .iter()
        .map(|a| {
            let sig = a
                .signature_top3
                .iter()
                .map(|s| format!("\"{}\"", json_escape(s)))
                .collect::<Vec<_>>()
                .join(", ");
            let frames = json_string_array(&a.top_frames);
            let normalized_frames = json_string_array(&a.normalized_frames);
            format!(
                "    {{\"attempt\": {}, \"result\": \"{}\", \"exit_code\": {}, \"signal\": \"{}\", \"sanitizer\": \"{}\", \"crash_kind\": \"{}\", \"crash_summary\": \"{}\", \"timeout\": {}, \"infra_oom\": {}, \"signature_top3\": [{}], \"top_frames\": [{}], \"normalized_frames\": [{}], \"normalized_frame_hash\": \"{}\"}}",
                a.attempt,
                a.result,
                option_i32_json_value(a.exit_code),
                json_escape(&a.signal),
                json_escape(&a.sanitizer),
                json_escape(&a.crash_kind),
                json_escape(&a.crash_summary),
                if a.timeout { "true" } else { "false" },
                if a.infra_oom { "true" } else { "false" },
                sig,
                frames,
                normalized_frames,
                json_escape(&a.normalized_frame_hash)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let summary = format!(
        "{{\n  \"schema_version\": \"1.1\",\n  \"triage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"input\": \"{}\",\n  \"repro_retries\": {},\n  \"timeout_sec\": {},\n  \"clean_count\": {},\n  \"crashed_count\": {},\n  \"timeout_count\": {},\n  \"infra_oom_count\": {},\n  \"signature_consistent\": {},\n  \"signature_basis\": \"normalized_frame_hash\",\n  \"normalized_frame_hash\": \"{}\",\n  \"crash_kind\": \"{}\",\n  \"sanitizer\": \"{}\",\n  \"signal\": \"{}\",\n  \"crash_summary\": \"{}\",\n  \"verdict\": \"{}\",\n  \"attempts\": [\n{}\n  ]\n}}\n",
        triage_id,
        target_label(target),
        json_escape(&input.display().to_string()),
        repro_retries,
        timeout_sec,
        clean_count,
        crashed_count,
        timeout_count,
        infra_oom_count,
        if signature_consistent { "true" } else { "false" },
        json_escape(representative.map(|a| a.normalized_frame_hash.as_str()).unwrap_or("")),
        json_escape(representative.map(|a| a.crash_kind.as_str()).unwrap_or("unknown")),
        json_escape(representative.map(|a| a.sanitizer.as_str()).unwrap_or("unknown")),
        json_escape(representative.map(|a| a.signal.as_str()).unwrap_or("unknown")),
        json_escape(representative.map(|a| a.crash_summary.as_str()).unwrap_or("")),
        verdict,
        attempts_json
    );
    fs::write(&summary_path, summary)
        .map_err(|e| format!("failed to write '{}': {e}", summary_path.display()))?;

    println!("[triage] done");
    println!("target: {}", target_label(target));
    println!("input: {}", input.display());
    println!("clean_count: {clean_count}");
    println!("crashed_count: {crashed_count}");
    println!("timeout_count: {timeout_count}");
    println!("infra_oom_count: {infra_oom_count}");
    println!("signature_consistent: {signature_consistent}");
    println!(
        "normalized_frame_hash: {}",
        representative
            .map(|a| a.normalized_frame_hash.as_str())
            .unwrap_or("")
    );
    println!("verdict: {verdict}");
    println!("summary: {}", summary_path.display());

    let valid_crashes = if verdict == "reproduced" { 1 } else { 0 };
    let new_crashes = if crashed_count > 0 { 1 } else { 0 };
    crate::metrics::record_metrics_event(
        app_paths,
        MetricEvent {
            ts: now_unix(),
            kind: "triage",
            total: attempts.len() as u64,
            errors: (crashed_count + timeout_count) as u64,
            successful_runs_proxy: 0,
            library_session_ok: 0,
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
) -> Result<TriageExecResult, String> {
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
    let mut merged = format!("{}\n{}", stdout, stderr);
    let exit_code = out.status.code();
    let signal = exit_signal(&out.status);

    if timeout_available && exit_code == Some(124) {
        return Ok(TriageExecResult {
            result: TriageResult::Timeout,
            output: merged,
            exit_code,
            signal,
        });
    }
    if out.status.success() {
        return Ok(TriageExecResult {
            result: TriageResult::Clean,
            output: merged,
            exit_code,
            signal,
        });
    }
    // OOM 137 triage 분기(DoS vs 인프라): v1은 infra_oom 힌트를 붙여 후속 triage/report에서 구분 가능하게 남긴다.
    if exit_code == Some(137) {
        merged = format!("infra_oom:exit_137\n{}", merged);
    }
    Ok(TriageExecResult {
        result: TriageResult::Crashed,
        output: merged,
        exit_code,
        signal,
    })
}

struct ParsedCrashLog {
    signal: String,
    sanitizer: String,
    crash_kind: String,
    crash_summary: String,
    signature_top3: Vec<String>,
    top_frames: Vec<String>,
    normalized_frames: Vec<String>,
    normalized_frame_hash: String,
    timeout: bool,
    infra_oom: bool,
}

fn parse_crash_log(
    output: &str,
    exit_code: Option<i32>,
    signal_number: Option<i32>,
    result_label: &str,
) -> ParsedCrashLog {
    if result_label == "clean" {
        return ParsedCrashLog {
            signal: "none".to_string(),
            sanitizer: "none".to_string(),
            crash_kind: "none".to_string(),
            crash_summary: String::new(),
            signature_top3: Vec::new(),
            top_frames: Vec::new(),
            normalized_frames: Vec::new(),
            normalized_frame_hash: String::new(),
            timeout: false,
            infra_oom: false,
        };
    }

    let sanitizer = extract_sanitizer(output);
    let signal = extract_signal(output, exit_code, signal_number);
    let timeout = result_label == "timeout" || contains_timeout(output);
    let infra_oom = exit_code == Some(137) || contains_oom(output);
    let crash_kind = classify_crash_kind(output, &sanitizer, &signal, timeout, infra_oom);
    let crash_summary = extract_crash_summary(output, &crash_kind);
    let top_frames = extract_top_frames(output);
    let normalized_frames = top_frames
        .iter()
        .map(|frame| normalize_stack_frame(frame))
        .filter(|frame| !frame.is_empty() && !is_ignored_frame(frame))
        .take(3)
        .collect::<Vec<_>>();
    let signature_top3 = if top_frames.is_empty() {
        extract_signature_top3(output)
    } else {
        top_frames.iter().take(3).cloned().collect()
    };
    let normalized_frame_hash = if normalized_frames.is_empty() {
        stable_hash_hex(&signature_top3.join("\n"))
    } else {
        stable_hash_hex(&normalized_frames.join("\n"))
    };

    ParsedCrashLog {
        signal,
        sanitizer,
        crash_kind,
        crash_summary,
        signature_top3,
        top_frames,
        normalized_frames,
        normalized_frame_hash,
        timeout,
        infra_oom,
    }
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

fn extract_sanitizer(output: &str) -> String {
    let lower = output.to_ascii_lowercase();
    if lower.contains("addresssanitizer") {
        "asan".to_string()
    } else if lower.contains("undefinedbehaviorsanitizer") || lower.contains("ubsan") {
        "ubsan".to_string()
    } else if lower.contains("memorysanitizer") {
        "msan".to_string()
    } else if lower.contains("threadsanitizer") {
        "tsan".to_string()
    } else if lower.contains("leaksanitizer") {
        "lsan".to_string()
    } else {
        "none".to_string()
    }
}

fn extract_signal(output: &str, exit_code: Option<i32>, signal_number: Option<i32>) -> String {
    if let Some(signal) = signal_number {
        return signal_name(signal).to_string();
    }
    let lower = output.to_ascii_lowercase();
    if lower.contains("sigsegv")
        || lower.contains("segmentation fault")
        || lower.contains("signal: 11")
    {
        "SIGSEGV".to_string()
    } else if lower.contains("sigabrt") || lower.contains("aborted") || lower.contains("signal: 6")
    {
        "SIGABRT".to_string()
    } else if lower.contains("sigbus") || lower.contains("bus error") || lower.contains("signal: 7")
    {
        "SIGBUS".to_string()
    } else if lower.contains("sigill")
        || lower.contains("illegal instruction")
        || lower.contains("signal: 4")
    {
        "SIGILL".to_string()
    } else if lower.contains("sigfpe")
        || lower.contains("floating point exception")
        || lower.contains("signal: 8")
    {
        "SIGFPE".to_string()
    } else if let Some(code) = exit_code.and_then(exit_code_signal_name) {
        code.to_string()
    } else {
        "none".to_string()
    }
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

fn signal_name(signal: i32) -> &'static str {
    match signal {
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

fn classify_crash_kind(
    output: &str,
    sanitizer: &str,
    signal: &str,
    timeout: bool,
    infra_oom: bool,
) -> String {
    let lower = output.to_ascii_lowercase();
    if timeout {
        return "timeout".to_string();
    }
    if infra_oom {
        return "infra_oom".to_string();
    }
    for kind in [
        "heap-buffer-overflow",
        "stack-buffer-overflow",
        "use-after-free",
        "null-dereference",
        "undefined-behavior",
    ] {
        if lower.contains(kind) {
            return kind.to_string();
        }
    }
    if sanitizer != "none" {
        sanitizer.to_string()
    } else if signal != "none" {
        signal.to_ascii_lowercase()
    } else if lower.contains("panic") {
        "panic".to_string()
    } else if lower.contains("abort") {
        "abort".to_string()
    } else if lower.contains("load_fail") || lower.contains("onnxruntimeerror") {
        "parser_or_runtime_error".to_string()
    } else {
        "manual_review".to_string()
    }
}

fn requires_manual_review(crash_kind: &str) -> bool {
    matches!(crash_kind, "manual_review" | "parser_or_runtime_error")
}

fn extract_crash_summary(output: &str, crash_kind: &str) -> String {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("summary:")
            || lower.contains("error:")
            || lower.contains("fatal")
            || lower.contains("runtimeerror")
            || lower.contains("load_fail")
            || lower.contains(crash_kind)
        {
            return trimmed.to_string();
        }
    }
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

fn extract_top_frames(output: &str) -> Vec<String> {
    let mut frames = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if is_stack_frame(trimmed) {
            frames.push(trimmed.to_string());
        }
        if frames.len() == 8 {
            break;
        }
    }
    frames
}

fn is_stack_frame(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.starts_with('#')
        || lower.starts_with("frame ")
        || lower.contains(" in ")
            && (lower.contains(" at ") || lower.contains(" from ") || lower.contains("!"))
}

fn normalize_stack_frame(line: &str) -> String {
    let mut s = line.trim().to_string();
    if let Some(rest) = s.strip_prefix('#') {
        let trimmed = rest
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start();
        s = trimmed.to_string();
    }
    s = strip_hex_addresses(&s);
    s = strip_file_line_suffix(&s);
    s = strip_symbol_offsets(&s);
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_hex_addresses(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|token| !token.starts_with("0x"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_file_line_suffix(input: &str) -> String {
    let mut out = input.to_string();
    if let Some(idx) = out.find(" at ") {
        out.truncate(idx);
    }
    if let Some(idx) = out.find(" from ") {
        out.truncate(idx);
    }
    out
}

fn strip_symbol_offsets(input: &str) -> String {
    input
        .split_whitespace()
        .map(|part| {
            if let Some(idx) = part.find("+0x") {
                &part[..idx]
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_ignored_frame(frame: &str) -> bool {
    let lower = frame.to_ascii_lowercase();
    lower.contains("addresssanitizer")
        || lower.contains("libasan")
        || lower.contains("libubsan")
        || lower.contains("libc.so")
        || lower.contains("libstdc++")
        || lower.contains("libgcc")
        || lower.contains("libfuzzer")
}

fn contains_timeout(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("timeout") || lower.contains("timed out") || lower.contains("hang")
}

fn contains_oom(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("infra_oom")
        || lower.contains("out of memory")
        || lower.contains("oom")
        || lower.contains("killed")
}

fn stable_hash_hex(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn json_string_array(items: &[String]) -> String {
    items
        .iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn option_i32_json_value(value: Option<i32>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
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
    use crate::json_utils::{extract_json_number_literal, extract_json_string_literal};

    // Backward-compatibility fixture: an old schema_version="1.1" triage summary
    // without a `deep_triage` nested object. Deep triage T2+ commits must keep
    // consumers tolerant of this shape (silent ignore of absent deep_triage).
    const SCHEMA_1_1_SAMPLE_SUMMARY: &str = r#"{
  "schema_version": "1.1",
  "triage_id": "1700000000000",
  "target": "onnx",
  "input": "data/runs/run-x/inputs/sample.onnx",
  "repro_retries": 3,
  "timeout_sec": 60,
  "clean_count": 0,
  "crashed_count": 3,
  "timeout_count": 0,
  "infra_oom_count": 0,
  "signature_consistent": true,
  "signature_basis": "normalized_frame_hash",
  "normalized_frame_hash": "abc123def456",
  "crash_kind": "heap-buffer-overflow",
  "sanitizer": "asan",
  "signal": "SIGSEGV",
  "crash_summary": "AddressSanitizer: heap-buffer-overflow",
  "verdict": "reproduced",
  "attempts": []
}
"#;

    #[test]
    fn schema_1_1_summary_top_level_fields_readable() {
        assert_eq!(
            extract_json_string_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "schema_version").as_deref(),
            Some("1.1")
        );
        assert_eq!(
            extract_json_string_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "verdict").as_deref(),
            Some("reproduced")
        );
        assert_eq!(
            extract_json_string_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "target").as_deref(),
            Some("onnx")
        );
        assert_eq!(
            extract_json_string_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "normalized_frame_hash")
                .as_deref(),
            Some("abc123def456")
        );
        assert_eq!(
            extract_json_string_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "crash_kind").as_deref(),
            Some("heap-buffer-overflow")
        );
        assert_eq!(
            extract_json_number_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "repro_retries").as_deref(),
            Some("3")
        );
    }

    #[test]
    fn schema_1_1_summary_has_no_deep_triage_object() {
        // Deep triage T1 audit policy: old schema_version="1.1" summaries
        // never carry a `deep_triage` nested object. Readers must tolerate
        // its absence (silent ignore, no panic, Option=None).
        assert!(
            extract_json_string_literal(SCHEMA_1_1_SAMPLE_SUMMARY, "deep_triage").is_none(),
            "old 1.1 summary must not contain deep_triage"
        );
    }
}
