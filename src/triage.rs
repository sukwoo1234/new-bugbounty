use std::{
    fs,
    path::Path,
};

use crate::common::{
    AppPaths, HarnessExecResult,
    command_exists, command_with_core_dump_off, now_unix, now_unix_millis,
};
use crate::json_utils::json_escape;
use crate::metrics::MetricEvent;
use crate::target::{TargetKind, target_label};

struct TriageAttempt {
    attempt: u32,
    result: String,
    signature_top3: Vec<String>,
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
    fs::create_dir_all(&triage_dir)
        .map_err(|e| format!("failed to create triage dir '{}': {e}", triage_dir.display()))?;

    let timeout_available = command_exists("timeout");
    let mut attempts = Vec::new();

    for attempt in 1..=repro_retries {
        let exec = execute_triage_subprocess(
            target,
            input,
            timeout_sec,
            timeout_available,
        )?;
        // harness exit 0 = 정상 종료 (clean), non-zero = 크래시 (crashed) per specs.md §3.1
        let (result_label, merged_output) = match exec {
            HarnessExecResult::Success(s) => ("clean".to_string(), s),
            HarnessExecResult::Failed(s) => ("crashed".to_string(), s),
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
    let clean_count = attempts.iter().filter(|a| a.result == "clean").count();
    let crashed_count = attempts.iter().filter(|a| a.result == "crashed").count();

    let mut signature_consistent = true;
    if let Some(first) = attempts.first().map(|a| &a.signature_top3) {
        signature_consistent = attempts.iter().all(|a| &a.signature_top3 == first);
    }

    // verdict per specs.md §4: reproduced = all crashed + sig consistent (High Confidence).
    // flaky = 1 in N crashed (specs line 144). not_reproduced = 0 crashed.
    let verdict = if timeout_count > 0 {
        "timeout"
    } else if crashed_count == attempts.len() && signature_consistent {
        "reproduced"
    } else if crashed_count == 0 {
        "not_reproduced"
    } else if !signature_consistent {
        "flaky_stack_mismatch"
    } else if crashed_count <= 1 {
        "flaky"
    } else {
        "partial"
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
        "{{\n  \"triage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"input\": \"{}\",\n  \"repro_retries\": {},\n  \"timeout_sec\": {},\n  \"clean_count\": {},\n  \"crashed_count\": {},\n  \"timeout_count\": {},\n  \"signature_consistent\": {},\n  \"verdict\": \"{}\",\n  \"attempts\": [\n{}\n  ]\n}}\n",
        triage_id,
        target_label(target),
        json_escape(&input.display().to_string()),
        repro_retries,
        timeout_sec,
        clean_count,
        crashed_count,
        timeout_count,
        if signature_consistent { "true" } else { "false" },
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
    println!("signature_consistent: {signature_consistent}");
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
