use std::{
    collections::VecDeque,
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
};

use crate::common::{
    artifact_contract, command_exists, command_with_core_dump_off, first_line, now_unix,
    now_unix_millis, shell_escape, AppPaths, ArtifactContract, HarnessExecResult,
};
use crate::metrics::{self, MetricEvent};
use crate::target::{
    collect_corpus_inputs, default_seed_dir, resolve_target_adapter, target_label, TargetKind,
};

#[derive(clap::ValueEnum, Clone, Debug, PartialEq, Eq)]
pub(crate) enum RunBackend {
    #[value(name = "local-harness")]
    LocalHarness,
    #[value(name = "aflpp")]
    Aflpp,
    #[value(name = "libfuzzer")]
    Libfuzzer,
}

struct EngineAdapter {
    backend_label: &'static str,
    cmd_env: &'static str,
}

#[derive(Clone)]
pub(crate) struct RunJob {
    pub(crate) id: usize,
    pub(crate) input: PathBuf,
}

#[derive(Default)]
struct RunStats {
    total: usize,
    success: usize,
    failed: usize,
    timeout: usize,
    retries: usize,
    library_session_ok: usize,
}

struct RunStatusCounts {
    total: usize,
    success: usize,
    failed: usize,
    timeout: usize,
    retries: usize,
}

struct EngineWorkerPlan {
    worker_id: usize,
    engine_cmd: String,
    worker_log_path: PathBuf,
}

struct EngineWorkerResult {
    outcome: EngineWorkerOutcome,
}

enum EngineWorkerOutcome {
    Success,
    Failed,
    Timeout,
}

pub(crate) fn run_fuzz_pipeline(
    app_paths: &AppPaths,
    target: &TargetKind,
    backend: &RunBackend,
    local: bool,
    corpus_dir: Option<&Path>,
    workers: usize,
    timeout_sec: u64,
    restart_limit: u32,
    max_jobs: Option<usize>,
) -> Result<(), String> {
    let artifact = artifact_contract(app_paths);
    if *backend != RunBackend::LocalHarness {
        return run_engine_backend(
            app_paths,
            &artifact,
            target,
            backend,
            local,
            corpus_dir,
            workers,
            timeout_sec,
            restart_limit,
        );
    }

    let corpus_dir = match corpus_dir {
        Some(path) => path.to_path_buf(),
        None if local => default_seed_dir(app_paths, target),
        None => app_paths.seeds_dir.clone(),
    };
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!("corpus_dir is invalid: {}", corpus_dir.display()));
    }

    // corpus reload: v1은 시작 시 코퍼스를 스냅샷으로 고정한다.
    // 재실행 루프 사이 신규 파일 반영은 차기 단계에서 "루프 간 재스캔"으로 확장한다.
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

    let run_id = now_unix_millis();
    let run_dir = artifact.runs_root.join(format!("run-{run_id}"));
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

    let workers = workers.max(1).min(
        queue
            .lock()
            .map_err(|_| "queue lock poisoned")?
            .len()
            .max(1),
    );
    let timeout_available = command_exists("timeout");

    println!("[run] start");
    println!("target: {}", target_label(target));
    println!("backend: {}", run_backend_label(backend));
    println!("local_mode: {}", local);
    println!("corpus_dir: {}", corpus_dir.display());
    println!("workers: {workers}");
    println!("timeout_sec: {}", timeout_sec);
    println!("restart_limit: {}", restart_limit);
    println!("run_dir: {}", run_dir.display());

    let mut handles = Vec::new();
    for _worker_id in 0..workers {
        let queue = Arc::clone(&queue);
        let stats = Arc::clone(&stats);
        let logs_dir = logs_dir.clone();
        let target = target.clone();

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

                let (result, retries_used, is_session_ok) = run_job_with_retry(
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
                if is_session_ok {
                    s.library_session_ok += 1;
                }
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

    // zombie fencing: 현재 구현은 파일 큐 대신 in-memory pop_front 단일 소유권으로 중복 처리 write를 방지한다.
    // file-queue 전환 시에는 결과 쓰기 전에 processing/<job_id> 존재 확인을 강제한다.
    let s = stats.lock().map_err(|_| "stats lock poisoned")?;
    let status_path = write_run_status(
        &run_dir,
        run_id,
        target,
        RunStatusCounts {
            total: s.total,
            success: s.success,
            failed: s.failed,
            timeout: s.timeout,
            retries: s.retries,
        },
        workers,
        timeout_sec,
        restart_limit,
    )?;

    println!("[run] done");
    println!("success: {}", s.success);
    println!("failed: {}", s.failed);
    println!("timeout: {}", s.timeout);
    println!("retries: {}", s.retries);
    println!("status: {}", status_path.display());

    metrics::record_metrics_event(
        app_paths,
        MetricEvent {
            ts: now_unix(),
            kind: "run",
            total: s.total as u64,
            errors: (s.failed + s.timeout) as u64,
            successful_runs_proxy: s.success as u64,
            library_session_ok: s.library_session_ok as u64,
            new_crashes: 0,
            valid_crashes: 0,
            total_crashes: 0,
        },
    )?;
    Ok(())
}

fn run_engine_backend(
    app_paths: &AppPaths,
    artifact: &ArtifactContract,
    target: &TargetKind,
    backend: &RunBackend,
    local: bool,
    corpus_dir: Option<&Path>,
    workers: usize,
    timeout_sec: u64,
    restart_limit: u32,
) -> Result<(), String> {
    if workers == 0 {
        return Err("workers must be >= 1".to_string());
    }

    let corpus_dir = match corpus_dir {
        Some(path) => path.to_path_buf(),
        None if local => default_seed_dir(app_paths, target),
        None => app_paths.seeds_dir.clone(),
    };
    if !corpus_dir.exists() || !corpus_dir.is_dir() {
        return Err(format!("corpus_dir is invalid: {}", corpus_dir.display()));
    }

    let run_id = now_unix_millis();
    let run_dir = artifact.runs_root.join(format!("run-{run_id}"));
    let logs_dir = run_dir.join("logs");
    fs::create_dir_all(&logs_dir)
        .map_err(|e| format!("failed to create run dir '{}': {e}", run_dir.display()))?;

    println!("[run] start");
    println!("target: {}", target_label(target));
    println!("backend: {}", run_backend_label(backend));
    println!("local_mode: {}", local);
    println!("corpus_dir: {}", corpus_dir.display());
    println!("workers: {}", workers);
    println!("timeout_sec: {}", timeout_sec);
    println!("restart_limit: {}", restart_limit);
    println!("run_dir: {}", run_dir.display());

    let mut worker_plans = Vec::with_capacity(workers);
    for worker_id in 1..=workers {
        let worker_log_path = logs_dir.join(format!("backend-engine-w{worker_id}.log"));
        let engine_cmd = build_engine_command(
            target,
            backend,
            &corpus_dir,
            &run_dir,
            workers,
            worker_id,
            timeout_sec,
            restart_limit,
            &worker_log_path,
        )?;
        println!("backend_engine_cmd[w{worker_id}]: {engine_cmd}");
        worker_plans.push(EngineWorkerPlan {
            worker_id,
            engine_cmd,
            worker_log_path,
        });
    }

    let mut success = 0usize;
    let mut failed = 0usize;
    let mut timeout = 0usize;

    let handles = worker_plans
        .into_iter()
        .map(|plan| thread::spawn(move || run_engine_worker(plan)))
        .collect::<Vec<_>>();

    for handle in handles {
        let result = match handle.join() {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("backend engine worker thread panicked".to_string()),
        };
        match result.outcome {
            EngineWorkerOutcome::Success => success += 1,
            EngineWorkerOutcome::Failed => failed += 1,
            EngineWorkerOutcome::Timeout => timeout += 1,
        }
    }

    let status_path = write_run_status(
        &run_dir,
        run_id,
        target,
        RunStatusCounts {
            total: workers,
            success,
            failed,
            timeout,
            retries: 0,
        },
        workers,
        timeout_sec,
        restart_limit,
    )?;

    println!("[run] done");
    println!("success: {success}");
    println!("failed: {failed}");
    println!("timeout: {timeout}");
    println!("retries: 0");
    println!("status: {}", status_path.display());

    metrics::record_metrics_event(
        app_paths,
        MetricEvent {
            ts: now_unix(),
            kind: "run",
            total: workers as u64,
            errors: (failed + timeout) as u64,
            successful_runs_proxy: success as u64,
            library_session_ok: 0,
            new_crashes: 0,
            valid_crashes: 0,
            total_crashes: 0,
        },
    )?;

    if failed == 0 && timeout == 0 {
        Ok(())
    } else {
        Err(format!(
            "backend '{}' engine command failed (failed={}, timeout={}, run_dir={})",
            run_backend_label(backend),
            failed,
            timeout,
            run_dir.display()
        ))
    }
}

fn run_engine_worker(plan: EngineWorkerPlan) -> Result<EngineWorkerResult, String> {
    let mut cmd = command_with_core_dump_off("bash");
    cmd.arg("-lc").arg(&plan.engine_cmd);
    let output = cmd.output().map_err(|e| {
        format!(
            "failed to execute backend engine command for worker {}: {e}",
            plan.worker_id
        )
    })?;
    let log_body = format!(
        "worker_id: {}\ncmd: {}\nexit_code: {:?}\n\n[stdout]\n{}\n\n[stderr]\n{}\n",
        plan.worker_id,
        plan.engine_cmd,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(&plan.worker_log_path, log_body)
        .map_err(|e| format!("failed to write '{}': {e}", plan.worker_log_path.display()))?;

    let exit_code = output.status.code().unwrap_or(1);
    let outcome = if output.status.success() {
        EngineWorkerOutcome::Success
    } else if exit_code == 124 {
        EngineWorkerOutcome::Timeout
    } else {
        EngineWorkerOutcome::Failed
    };
    Ok(EngineWorkerResult { outcome })
}

fn build_engine_command(
    target: &TargetKind,
    backend: &RunBackend,
    corpus_dir: &Path,
    run_dir: &Path,
    workers: usize,
    worker_id: usize,
    timeout_sec: u64,
    restart_limit: u32,
    worker_log_path: &Path,
) -> Result<String, String> {
    let engine = resolve_engine_adapter(backend)?;
    let target_adapter = resolve_target_adapter(target);
    let docker_user_flag = current_docker_user_flag();
    let docker_hardening_flags = docker_hardening_flags();
    let docker_readonly_flags = docker_readonly_flags();
    let workdir_abs =
        absolute_path(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let corpus_dir_abs = absolute_path(corpus_dir);
    let run_dir_abs = absolute_path(run_dir);
    let template = std::env::var(engine.cmd_env).map_err(|_| {
        format!(
            "{} is not set; provide backend command template. example: {}='echo run {{target}} {{corpus_dir}}; true'",
            engine.cmd_env, engine.cmd_env
        )
    })?;
    if template.trim().is_empty() {
        return Err(format!("{} is empty", engine.cmd_env));
    }

    let cmd = template
        .replace("{target}", target_adapter.target_label)
        .replace("{backend}", engine.backend_label)
        .replace("{corpus_dir}", &shell_escape(corpus_dir))
        .replace("{workers}", &workers.to_string())
        .replace("{worker_id}", &worker_id.to_string())
        .replace("{timeout_sec}", &timeout_sec.to_string())
        .replace("{restart_limit}", &restart_limit.to_string())
        .replace("{docker_user_flag}", &docker_user_flag)
        .replace("{docker_hardening_flags}", docker_hardening_flags)
        .replace("{docker_readonly_flags}", docker_readonly_flags)
        .replace("{run_dir}", &shell_escape(run_dir))
        .replace("{workdir_abs}", &shell_escape(&workdir_abs))
        .replace("{corpus_dir_abs}", &shell_escape(&corpus_dir_abs))
        .replace("{run_dir_abs}", &shell_escape(&run_dir_abs))
        .replace("{container_workdir}", "/work")
        .replace("{container_corpus_dir}", "/corpus")
        .replace("{container_run_dir}", "/out")
        .replace("{workdir}", &shell_escape(&workdir_abs))
        .replace("{worker_log}", &shell_escape(worker_log_path));

    Ok(cmd)
}

fn current_docker_user_flag() -> String {
    let uid = read_id_output("-u");
    let gid = read_id_output("-g");
    match (uid, gid) {
        (Some(uid), Some(gid)) => format!("--user {uid}:{gid}"),
        _ => String::new(),
    }
}

fn docker_hardening_flags() -> &'static str {
    "--network none --memory 4g --cpus 2 --pids-limit 512"
}

fn docker_readonly_flags() -> &'static str {
    "--read-only --tmpfs /tmp:rw,size=1g --tmpfs /dev/shm:rw,size=1g"
}

fn absolute_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn read_id_output(flag: &str) -> Option<String> {
    let out = Command::new("id").arg(flag).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_engine_adapter(backend: &RunBackend) -> Result<EngineAdapter, String> {
    match backend {
        RunBackend::Aflpp => Ok(EngineAdapter {
            backend_label: "aflpp",
            cmd_env: "TOOL_AFLPP_CMD",
        }),
        RunBackend::Libfuzzer => Ok(EngineAdapter {
            backend_label: "libfuzzer",
            cmd_env: "TOOL_LIBFUZZER_CMD",
        }),
        RunBackend::LocalHarness => {
            Err("internal error: local-harness should not use engine command".to_string())
        }
    }
}

fn run_job_with_retry(
    job: &RunJob,
    target: &TargetKind,
    timeout_sec: u64,
    restart_limit: u32,
    timeout_available: bool,
    logs_dir: &Path,
) -> Result<(HarnessExecResult, usize, bool), String> {
    let attempts = restart_limit + 1;
    let mut last = HarnessExecResult::Failed("not executed".to_string());
    let mut last_session_ok = false;
    let mut retries_used = 0usize;

    for attempt in 1..=attempts {
        let (result, is_session_ok) =
            execute_harness_subprocess(job, target, timeout_sec, timeout_available)?;
        last_session_ok = is_session_ok;
        write_job_log(logs_dir, job, attempt, &result)?;
        match result {
            HarnessExecResult::Success(_) => return Ok((result, retries_used, is_session_ok)),
            other => last = other,
        }
        if attempt < attempts {
            retries_used += 1;
        }
    }

    Ok((last, retries_used, last_session_ok))
}

pub(crate) fn execute_harness_subprocess(
    job: &RunJob,
    target: &TargetKind,
    timeout_sec: u64,
    timeout_available: bool,
) -> Result<(HarnessExecResult, bool), String> {
    let exe = std::env::current_exe().map_err(|e| format!("failed to resolve current exe: {e}"))?;
    let target_name = target_label(target).to_string();
    let input = job.input.display().to_string();

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
    let is_session_ok = stdout
        .lines()
        .any(|l| l.trim() == "library_outcome: session_ok");
    let summary = format!(
        "stdout: {}\nstderr: {}",
        first_line(&stdout),
        first_line(&stderr)
    );

    if timeout_available && out.status.code() == Some(124) {
        return Ok((HarnessExecResult::Timeout(summary), is_session_ok));
    }
    if out.status.success() {
        return Ok((HarnessExecResult::Success(summary), is_session_ok));
    }
    Ok((HarnessExecResult::Failed(summary), is_session_ok))
}

pub(crate) fn write_job_log(
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

fn run_backend_label(backend: &RunBackend) -> &'static str {
    match backend {
        RunBackend::LocalHarness => "local-harness",
        RunBackend::Aflpp => "aflpp",
        RunBackend::Libfuzzer => "libfuzzer",
    }
}

fn write_run_status(
    run_dir: &Path,
    run_id: u128,
    target: &TargetKind,
    counts: RunStatusCounts,
    workers: usize,
    timeout_sec: u64,
    restart_limit: u32,
) -> Result<PathBuf, String> {
    let status_path = run_dir.join("status.json");
    let status_json = format!(
        "{{\n  \"run_id\": \"{}\",\n  \"target\": \"{}\",\n  \"total\": {},\n  \"success\": {},\n  \"failed\": {},\n  \"timeout\": {},\n  \"retries\": {},\n  \"workers\": {},\n  \"timeout_sec\": {},\n  \"restart_limit\": {}\n}}\n",
        run_id,
        target_label(target),
        counts.total,
        counts.success,
        counts.failed,
        counts.timeout,
        counts.retries,
        workers,
        timeout_sec,
        restart_limit
    );
    fs::write(&status_path, status_json)
        .map_err(|e| format!("failed to write '{}': {e}", status_path.display()))?;
    Ok(status_path)
}
