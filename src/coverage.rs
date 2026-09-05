use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::common::{
    artifact_contract, command_exists, now_unix, now_unix_millis, output_with_deadline,
    validate_max_jobs, validate_timeout_sec, AppPaths, HarnessExecResult,
};
use crate::json_utils::{extract_json_string_literal, extract_json_u64_field, json_escape};
use crate::run::{execute_harness_subprocess, write_job_log, RunJob};
use crate::target::{collect_corpus_inputs, default_seed_dir, target_label, TargetKind};

// The env var naming the pinned instrumented coverage command, selected per target so
// `coverage --target <t>` runs the right one. Mirrors the external-harness env keys in
// src/target.rs (TOOL_GGUF_HARNESS_CMD / ...). ONNX keeps its historical name.
fn coverage_cmd_env_key(target: &TargetKind) -> &'static str {
    match target {
        TargetKind::Gguf => "TOOL_COVERAGE_GGUF_CMD",
        TargetKind::Onnx => "TOOL_COVERAGE_ONNX_CMD",
        TargetKind::Safetensors => "TOOL_COVERAGE_SAFETENSORS_CMD",
    }
}

pub(crate) fn run_coverage_job(
    app_paths: &AppPaths,
    target: &TargetKind,
    corpus_dir: Option<&Path>,
    timeout_sec: u64,
    max_jobs: Option<usize>,
) -> Result<(), String> {
    // A29: same zero-budget hole as the run pipeline.
    let timeout_sec = validate_timeout_sec(timeout_sec)?;
    let max_jobs = validate_max_jobs(max_jobs)?;

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

    // V2 real-coverage path (opt-in, env-gated). When the per-target coverage command
    // env var is set (coverage_cmd_env_key),
    // run the pinned instrumented coverage command and emit a schema 2.0 summary from
    // its coverage.json. When unset, fall through to the existing proxy replay below
    // so baseline workflows keep working unchanged.
    if let Some(cmd) = std::env::var(coverage_cmd_env_key(target))
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        let staged_corpus_dir =
            stage_real_coverage_inputs(&coverage_dir, &inputs, &format!("coverage-{coverage_id}"))?;
        let result = run_real_coverage(
            &coverage_dir,
            &staged_corpus_dir,
            &corpus_dir,
            &cmd,
            format!("coverage-{coverage_id}"),
            timeout_sec,
        );
        if let Err(err) = fs::remove_dir_all(&staged_corpus_dir) {
            eprintln!(
                "[coverage] warning: failed to remove staged corpus '{}': {err}",
                staged_corpus_dir.display()
            );
        }
        return result;
    }

    let timeout_available = command_exists("timeout");
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut timeout = 0usize;
    let mut rejected = 0usize;

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
            // G3: this input never reached the library, so it is not a coverage failure
            HarnessExecResult::Rejected(_) => rejected += 1,
        }
    }

    let total = success + failed + timeout + rejected;
    let summary_path = coverage_dir.join("summary.json");
    let summary = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"coverage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"corpus_dir\": \"{}\",\n  \"timeout_sec\": {},\n  \"total\": {},\n  \"success\": {},\n  \"failed\": {},\n  \"timeout\": {},\n  \"rejected\": {},\n  \"coverage_proxy\": {{\n    \"success_ratio\": {:.4}\n  }},\n  \"generated_at\": {}\n}}\n",
        coverage_id,
        target_label(target),
        json_escape(&corpus_dir.display().to_string()),
        timeout_sec,
        total,
        success,
        failed,
        timeout,
        rejected,
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

// Copy only the already-selected inputs into a private directory. The real coverage
// scripts discover files from CORPUS_DIR themselves, so passing the original corpus
// directory would silently undo --max-jobs (and would let files added after the snapshot
// enter the run). Hard links keep the normal same-filesystem case cheap; copying is the
// fallback for a corpus on another filesystem.
fn stage_real_coverage_inputs(
    coverage_dir: &Path,
    inputs: &[PathBuf],
    run_id: &str,
) -> Result<PathBuf, String> {
    let parent = coverage_dir
        .parent()
        .ok_or_else(|| format!("coverage dir has no parent: {}", coverage_dir.display()))?;
    let staged_dir = parent.join(format!(".{run_id}-inputs"));
    fs::create_dir(&staged_dir).map_err(|e| {
        format!(
            "failed to create staged coverage corpus '{}': {e}",
            staged_dir.display()
        )
    })?;

    for input in inputs {
        let name = match input.file_name() {
            Some(name) => name,
            None => {
                let _ = fs::remove_dir_all(&staged_dir);
                return Err(format!(
                    "coverage input has no file name: {}",
                    input.display()
                ));
            }
        };
        let dest = staged_dir.join(name);
        let is_symlink = fs::symlink_metadata(input)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink || fs::hard_link(input, &dest).is_err() {
            if let Err(e) = fs::copy(input, &dest) {
                let _ = fs::remove_dir_all(&staged_dir);
                return Err(format!(
                    "failed to stage coverage input '{}' as '{}': {e}",
                    input.display(),
                    dest.display()
                ));
            }
        }
    }

    Ok(staged_dir)
}

// Run the pinned instrumented coverage command, then parse the coverage.json it
// produces and emit a schema 2.0 summary.json beside it. The command receives the
// selected corpus via CORPUS_DIR and the original corpus via SOURCE_CORPUS_DIR. The
// command itself is bounded in-process so a missing `timeout` binary cannot make a
// coverage run hang forever. Kept fully separate from the proxy replay and from the
// run -> triage -> report pipeline.
fn run_real_coverage(
    coverage_dir: &Path,
    selected_corpus_dir: &Path,
    source_corpus_dir: &Path,
    cmd: &str,
    run_id: String,
    timeout_sec: u64,
) -> Result<(), String> {
    println!("[coverage] real instrumented coverage (env-gated command)");
    let mut command = Command::new("bash");
    command
        .arg("-lc")
        .arg(cmd)
        .env("OUT_DIR", coverage_dir)
        .env("CORPUS_DIR", selected_corpus_dir)
        .env("SOURCE_CORPUS_DIR", source_corpus_dir);
    let (output, timed_out) = output_with_deadline(command, timeout_sec)
        .map_err(|e| format!("failed to spawn coverage command: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    if timed_out {
        return Err(format!(
            "coverage command timed out after {timeout_sec} seconds"
        ));
    }
    if !output.status.success() {
        return Err(format!("coverage command failed ({})", output.status));
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
// coverage command (coverage_cmd_env_key, per target) produces a coverage.json. Every
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
    use std::sync::Mutex;

    static COVERAGE_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn unique_test_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tool-coverage-{name}-{}-{}",
            std::process::id(),
            now_unix_millis()
        ));
        fs::create_dir_all(&root).expect("test root should be created");
        root
    }

    fn only_summary_json(app_paths: &AppPaths) -> String {
        let coverage_root = app_paths.data_dir.join("coverage");
        let entries = fs::read_dir(&coverage_root)
            .expect("coverage root should exist")
            .collect::<Result<Vec<_>, _>>()
            .expect("coverage dirs should be readable");
        assert_eq!(entries.len(), 1, "expected one coverage dir");
        fs::read_to_string(entries[0].path().join("summary.json"))
            .expect("summary.json should be readable")
    }

    // The real-coverage command is selected per target: onnx keeps its name for
    // backward compatibility, and gguf/safetensors get their own. Before this, the
    // ONNX key was read regardless of --target, so `coverage --target safetensors`
    // silently ran the onnx command.
    #[test]
    fn coverage_env_key_is_per_target() {
        assert_eq!(coverage_cmd_env_key(&TargetKind::Onnx), "TOOL_COVERAGE_ONNX_CMD");
        assert_eq!(coverage_cmd_env_key(&TargetKind::Gguf), "TOOL_COVERAGE_GGUF_CMD");
        assert_eq!(
            coverage_cmd_env_key(&TargetKind::Safetensors),
            "TOOL_COVERAGE_SAFETENSORS_CMD"
        );
    }

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

    #[test]
    fn real_coverage_honors_max_jobs_by_passing_selected_inputs() {
        let _env_guard = COVERAGE_ENV_LOCK
            .lock()
            .expect("coverage env lock poisoned");
        let root = unique_test_root("max-jobs");
        let app_paths = AppPaths::prepare(&root.join("data"), &root.join("seeds"))
            .expect("app paths should prepare");
        let corpus = root.join("corpus");
        fs::create_dir_all(&corpus).expect("corpus should be created");
        for name in ["a.onnx", "b.onnx", "c.onnx"] {
            fs::write(corpus.join(name), b"seed").expect("seed should be written");
        }

        let _cmd = EnvVarGuard::set(
            "TOOL_COVERAGE_ONNX_CMD",
            r#"count="$(find "$CORPUS_DIR" -type f -name '*.onnx' | wc -l | tr -d ' ')"
cat > "$OUT_DIR/coverage.json" <<EOF
{"schema_version":"2.0","coverage_kind":"line","instrumentation":"test","covered_lines":${count},"total_lines":10}
EOF"#,
        );

        run_coverage_job(&app_paths, &TargetKind::Onnx, Some(&corpus), 30, Some(1))
            .expect("coverage run should succeed");
        let summary = only_summary_json(&app_paths);
        assert!(
            summary.contains("\"covered_lines\": 1"),
            "real coverage should expose only the selected max_jobs input; summary was:\n{summary}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn real_coverage_timeout_sec_bounds_the_external_command() {
        let _env_guard = COVERAGE_ENV_LOCK
            .lock()
            .expect("coverage env lock poisoned");
        let root = unique_test_root("timeout");
        let app_paths = AppPaths::prepare(&root.join("data"), &root.join("seeds"))
            .expect("app paths should prepare");
        let corpus = root.join("corpus");
        fs::create_dir_all(&corpus).expect("corpus should be created");
        fs::write(corpus.join("a.onnx"), b"seed").expect("seed should be written");

        let _cmd = EnvVarGuard::set(
            "TOOL_COVERAGE_ONNX_CMD",
            r#"sleep 2
cat > "$OUT_DIR/coverage.json" <<'EOF'
{"schema_version":"2.0","coverage_kind":"line","instrumentation":"test","covered_lines":1,"total_lines":1}
EOF"#,
        );

        let err = run_coverage_job(&app_paths, &TargetKind::Onnx, Some(&corpus), 1, None)
            .expect_err("timeout_sec should bound real coverage commands");
        assert!(
            err.contains("timed out") || err.contains("timeout"),
            "expected timeout error, got: {err}"
        );

        let _ = fs::remove_dir_all(root);
    }
}
