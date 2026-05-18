use std::{fs, path::Path};

use crate::common::{
    artifact_contract, command_exists, now_unix, now_unix_millis, AppPaths, HarnessExecResult,
};
use crate::json_utils::json_escape;
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
