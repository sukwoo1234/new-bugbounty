use std::{fs, path::Path, process::Command};

use crate::common::{
    artifact_contract, command_exists, now_unix, now_unix_millis, AppPaths, HarnessExecResult,
};
use crate::json_utils::{extract_json_string_literal, extract_json_u64_field, json_escape};
use crate::run::{execute_harness_subprocess, write_job_log, RunJob};
use crate::target::{collect_corpus_inputs, default_seed_dir, target_label, TargetKind};

pub(crate) fn run_coverage_job(
    app_paths: &AppPaths,
    target: &TargetKind,
    corpus_dir: Option<&Path>,
    timeout_sec: u64,
    max_jobs: Option<usize>,
) -> Result<(), String> {
    let artifact = artifact_contract(app_paths);
    let corpus_dir = match corpus_dir {
        Some(path) => path.to_path_buf(),
        None => default_seed_dir(app_paths, target),
    };
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!("corpus_dir is invalid: {}", corpus_dir.display()));
    }

    let mut inputs = collect_corpus_inputs(&corpus_dir, target)?;
    if inputs.is_empty() {
        return Err(format!(
            "no input files found for target '{}' in {}",
            target_label(target),
            corpus_dir.display()
        ));
    }
    if let Some(max_jobs) = max_jobs {
        inputs.truncate(max_jobs);
    }

    let coverage_id = now_unix_millis();
    let coverage_dir = artifact
        .coverage_root
        .join(format!("coverage-{coverage_id}"));
    let logs_dir = coverage_dir.join("logs");
    fs::create_dir_all(&logs_dir).map_err(|e| {
        format!(
            "failed to create coverage dir '{}': {e}",
            coverage_dir.display()
        )
    })?;

    println!("[coverage] start");
    println!("target: {}", target_label(target));
    println!("corpus_dir: {}", corpus_dir.display());
    println!("timeout_sec: {}", timeout_sec);
    println!("coverage_dir: {}", coverage_dir.display());

    // V2 real-coverage path (opt-in, env-gated). When TOOL_COVERAGE_ONNX_CMD is set,
    // run the pinned instrumented coverage command and emit a schema 2.0 summary from
    // its coverage.json. When unset, fall through to the existing proxy replay below
    // so baseline workflows keep working unchanged.
    if let Some(cmd) = std::env::var("TOOL_COVERAGE_ONNX_CMD")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return run_real_coverage(
            &coverage_dir,
            &corpus_dir,
            &cmd,
            format!("coverage-{coverage_id}"),
        );
    }

    let timeout_available = command_exists("timeout");
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut timeout = 0usize;

    for (i, input) in inputs.iter().enumerate() {
        let job = RunJob {
            id: i,
            input: input.clone(),
        };
        let (result, _is_session_ok) =
            execute_harness_subprocess(&job, target, timeout_sec, timeout_available)?;
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
        target_label(target),
        json_escape(&corpus_dir.display().to_string()),
        timeout_sec,
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

// Run the pinned instrumented coverage command, then parse the coverage.json it
// produces and emit a schema 2.0 summary.json beside it. The command receives the
// output dir and corpus dir via OUT_DIR / CORPUS_DIR env vars. Kept fully separate
// from the proxy replay and from the run -> triage -> report pipeline.
fn run_real_coverage(
    coverage_dir: &Path,
    corpus_dir: &Path,
    cmd: &str,
    run_id: String,
) -> Result<(), String> {
    println!("[coverage] real instrumented coverage via TOOL_COVERAGE_ONNX_CMD");
    let status = Command::new("bash")
        .arg("-lc")
        .arg(cmd)
        .env("OUT_DIR", coverage_dir)
        .env("CORPUS_DIR", corpus_dir)
        .status()
        .map_err(|e| format!("failed to spawn coverage command: {e}"))?;
    if !status.success() {
        return Err(format!("coverage command failed ({status})"));
    }

    let cov_json_path = coverage_dir.join("coverage.json");
    let json = fs::read_to_string(&cov_json_path).map_err(|e| {
        format!(
            "coverage command did not produce {}: {e}",
            cov_json_path.display()
        )
    })?;
    let coverage = parse_coverage_artifact(&json)
        .ok_or_else(|| format!("could not parse coverage artifact at {}", cov_json_path.display()))?;

    let summary = render_coverage_summary_v2(&coverage, &run_id, now_unix());
    let summary_path = coverage_dir.join("summary.json");
    fs::write(&summary_path, summary)
        .map_err(|e| format!("failed to write '{}': {e}", summary_path.display()))?;

    println!("[coverage] done (real)");
    println!("coverage.json: {}", cov_json_path.display());
    println!("summary: {}", summary_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// V2 real-coverage artifact (schema 2.0). Populated only when an instrumented
// coverage command (TOOL_COVERAGE_ONNX_CMD) produces a coverage.json. Every
// numeric field is Option: absent in the source artifact stays absent here --
// it is NEVER substituted with 0 (no fake values).
// ---------------------------------------------------------------------------
#[derive(Debug, Default, PartialEq)]
struct CoverageArtifact {
    schema_version: Option<String>,
    coverage_kind: Option<String>,
    instrumentation: Option<String>,
    toolchain_version: Option<String>,
    covered_lines: Option<u64>,
    total_lines: Option<u64>,
    covered_functions: Option<u64>,
    total_functions: Option<u64>,
    covered_edges: Option<u64>,
    total_edges: Option<u64>,
}

fn parse_coverage_artifact(json: &str) -> Option<CoverageArtifact> {
    if json.trim().is_empty() {
        return None;
    }
    Some(CoverageArtifact {
        schema_version: extract_json_string_literal(json, "schema_version"),
        coverage_kind: extract_json_string_literal(json, "coverage_kind"),
        instrumentation: extract_json_string_literal(json, "instrumentation"),
        toolchain_version: extract_json_string_literal(json, "toolchain_version"),
        covered_lines: extract_json_u64_field(json, "covered_lines"),
        total_lines: extract_json_u64_field(json, "total_lines"),
        covered_functions: extract_json_u64_field(json, "covered_functions"),
        total_functions: extract_json_u64_field(json, "total_functions"),
        covered_edges: extract_json_u64_field(json, "covered_edges"),
        total_edges: extract_json_u64_field(json, "total_edges"),
    })
}

// Emit one coverage metric group (covered_<name>s / total_<name>s / <name>_coverage)
// only when BOTH covered and total are present. A missing metric is omitted entirely;
// the percentage is computed (never faked) and only added when total > 0.
fn push_metric_group(fields: &mut Vec<String>, name: &str, covered: Option<u64>, total: Option<u64>) {
    let (c, t) = match (covered, total) {
        (Some(c), Some(t)) => (c, t),
        _ => return,
    };
    fields.push(format!("  \"covered_{name}s\": {c}"));
    fields.push(format!("  \"total_{name}s\": {t}"));
    if t > 0 {
        let pct = (c as f64) / (t as f64) * 100.0;
        fields.push(format!("  \"{name}_coverage\": {pct:.4}"));
    }
}

fn render_coverage_summary_v2(
    artifact: &CoverageArtifact,
    run_id: &str,
    generated_at: u64,
) -> String {
    let mut fields: Vec<String> = Vec::new();
    fields.push("  \"schema_version\": \"2.0\"".to_string());
    fields.push(format!("  \"run_id\": \"{}\"", json_escape(run_id)));
    if let Some(kind) = &artifact.coverage_kind {
        fields.push(format!("  \"coverage_kind\": \"{}\"", json_escape(kind)));
    }
    if let Some(instr) = &artifact.instrumentation {
        fields.push(format!("  \"instrumentation\": \"{}\"", json_escape(instr)));
    }
    if let Some(tv) = &artifact.toolchain_version {
        fields.push(format!("  \"toolchain_version\": \"{}\"", json_escape(tv)));
    }
    push_metric_group(&mut fields, "line", artifact.covered_lines, artifact.total_lines);
    push_metric_group(
        &mut fields,
        "function",
        artifact.covered_functions,
        artifact.total_functions,
    );
    push_metric_group(&mut fields, "edge", artifact.covered_edges, artifact.total_edges);
    fields.push(format!("  \"generated_at\": {generated_at}"));
    format!("{{\n{}\n}}\n", fields.join(",\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_coverage_artifact_omits_absent_metrics_no_fake_values() {
        // edge-only artifact (e.g. sancov): has edges, but NO line/function metrics.
        let json = r#"{
          "schema_version": "2.0",
          "coverage_kind": "edge",
          "instrumentation": "sancov-libfuzzer",
          "covered_edges": 311,
          "total_edges": 311
        }"#;
        let art = parse_coverage_artifact(json).expect("valid artifact should parse");
        assert_eq!(art.covered_edges, Some(311));
        assert_eq!(art.total_edges, Some(311));
        // absent metrics must be None, never Some(0) -- no fake values
        assert_eq!(art.covered_lines, None);
        assert_eq!(art.total_lines, None);
        assert_eq!(art.covered_functions, None);

        let summary = render_coverage_summary_v2(&art, "cov-test-1", 1_780_000_000);
        assert!(summary.contains("\"schema_version\": \"2.0\""));
        assert!(summary.contains("\"edge_coverage\""));
        assert!(summary.contains("\"covered_edges\": 311"));
        // omitted, not zero-filled
        assert!(!summary.contains("line_coverage"));
        assert!(!summary.contains("covered_lines"));
    }

    #[test]
    fn render_coverage_summary_v2_emits_present_line_and_function_metrics() {
        let json = r#"{"schema_version":"2.0","coverage_kind":"line_function",
          "instrumentation":"llvm-source-cov","covered_lines":20394,"total_lines":143252,
          "covered_functions":2033,"total_functions":10750}"#;
        let art = parse_coverage_artifact(json).expect("valid artifact should parse");
        let s = render_coverage_summary_v2(&art, "cov-x", 1_780_000_000);
        assert!(s.contains("\"line_coverage\""));
        assert!(s.contains("\"covered_lines\": 20394"));
        assert!(s.contains("\"total_lines\": 143252"));
        assert!(s.contains("\"function_coverage\""));
        // no edge data in this artifact -> omitted
        assert!(!s.contains("\"edge_coverage\""));
        assert!(!s.contains("covered_edges"));
    }
}
