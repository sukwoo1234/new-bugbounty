use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::dashboard_charts::{
    build_crash_intake_series, build_run_result_series, build_throughput_proxy_series,
    build_triage_verdict_breakdown, collect_recent_run_statuses, collect_recent_triage_verdicts,
    CrashIntakePoint, RunResultPoint, ThroughputProxyPoint, VerdictBreakdownPoint,
    CHART_SERIES_LIMIT,
};

use crate::common::{artifact_contract, has_ext, now_unix, AppPaths};
use crate::json_utils::{
    extract_first_signature_top1, extract_json_number_literal, extract_json_string_literal,
};
use crate::target::{default_seed_dir, resolve_target_adapter, TargetKind};

#[derive(Clone, Debug)]
pub(crate) struct DashboardSnapshot {
    pub(crate) generated_at: u64,
    pub(crate) data_dir: String,
    pub(crate) seeds_dir: String,
    pub(crate) runs_count: usize,
    pub(crate) triage_count: usize,
    pub(crate) report_count: usize,
    pub(crate) latest_run: String,
    pub(crate) latest_triage: String,
    pub(crate) latest_report: String,
    pub(crate) metrics_exists: bool,
    pub(crate) successful_runs_per_hour_proxy: String,
    pub(crate) new_crashes_per_hour: String,
    pub(crate) valid_crash_ratio: String,
    pub(crate) valid_crash_ratio_status: String,
    pub(crate) valid_crash_ratio_source: String,
    pub(crate) valid_crashes: String,
    pub(crate) total_crashes: String,
    pub(crate) triage_summary_count: String,
    pub(crate) global_error_rate_5m: String,
    pub(crate) latest_valid_triage: String,
    pub(crate) latest_valid_input: String,
    pub(crate) latest_valid_signature: String,
    pub(crate) latest_valid_crash_kind: String,
    pub(crate) latest_valid_sanitizer: String,
    pub(crate) latest_valid_signal: String,
    pub(crate) latest_valid_normalized_frame_hash: String,
    pub(crate) latest_valid_signature_basis: String,
    pub(crate) latest_valid_crash_summary: String,
    pub(crate) latest_valid_summary: String,
    pub(crate) latest_valid_report: String,
    pub(crate) latest_valid_manifest: String,
    pub(crate) latest_valid_bundle: String,
    pub(crate) latest_suggested_severity: String,
    pub(crate) latest_severity_confidence: String,
    pub(crate) latest_suggested_cvss_vector: String,
    pub(crate) recent_triage_ids: Vec<String>,
    pub(crate) recent_report_ids: Vec<String>,
    pub(crate) recent_coverage_ids: Vec<String>,
    pub(crate) seeds_onnx_count: usize,
    pub(crate) seeds_gguf_count: usize,
    pub(crate) seeds_safetensors_count: usize,
    pub(crate) seeds_total_count: usize,
    pub(crate) coverage_count: usize,
    pub(crate) latest_coverage: String,
    pub(crate) latest_coverage_summary: String,
    pub(crate) latest_export_id: String,
    pub(crate) latest_export_path: String,
    pub(crate) latest_export_summary: String,
    pub(crate) latest_mutation_batch_id: String,
    pub(crate) latest_mutation_manifest_path: String,
    pub(crate) latest_mutation_target: String,
    pub(crate) latest_mutation_count: String,
    pub(crate) latest_run_target: String,
    pub(crate) latest_run_backend: String,
    pub(crate) latest_run_total: String,
    pub(crate) latest_run_success: String,
    pub(crate) latest_run_failed: String,
    pub(crate) latest_run_timeout: String,
    pub(crate) triage_verdict_reproduced: usize,
    pub(crate) triage_verdict_manual_review: usize,
    pub(crate) triage_verdict_not_reproduced: usize,
    pub(crate) triage_verdict_timeout: usize,
    pub(crate) triage_verdict_infra_oom: usize,
    pub(crate) triage_verdict_flaky: usize,
    pub(crate) triage_verdict_other: usize,
    pub(crate) run_state: String,
    pub(crate) latest_triage_verdict: String,
    pub(crate) latest_triage_target: String,
    pub(crate) latest_run_updated_at: String,
    pub(crate) latest_triage_updated_at: String,
    pub(crate) latest_report_updated_at: String,
    pub(crate) latest_export_updated_at: String,
    pub(crate) latest_mutation_updated_at: String,
    pub(crate) latest_mutation_source_corpus: String,
    pub(crate) latest_mutation_validation_summary: String,
    pub(crate) run_result_series: Vec<RunResultPoint>,
    pub(crate) throughput_proxy_series: Vec<ThroughputProxyPoint>,
    pub(crate) crash_intake_series: Vec<CrashIntakePoint>,
    pub(crate) triage_verdict_breakdown: Vec<VerdictBreakdownPoint>,
}

struct ReproducedTriageView {
    triage_id: String,
    input: String,
    signature_top1: String,
    crash_kind: String,
    sanitizer: String,
    signal: String,
    normalized_frame_hash: String,
    signature_basis: String,
    crash_summary: String,
    summary_path: String,
}

#[derive(Default)]
struct ReportSeverityView {
    suggested_severity: String,
    confidence: String,
    suggested_cvss_vector: String,
}

/// `tool list` 용. kind 별로 최근 N개 ID 목록을 돌려준다.
pub(crate) fn list_recent_ids(
    app_paths: &AppPaths,
    kind: &str,
    limit: usize,
) -> Result<Vec<String>, String> {
    let artifact = artifact_contract(app_paths);
    let (root, prefix) = match kind {
        "runs" => (artifact.runs_root, "run-"),
        "triages" => (artifact.triage_root, "triage-"),
        "reports" => (artifact.reports_root, "report-"),
        "coverage" => (artifact.coverage_root, "coverage-"),
        other => {
            return Err(format!(
                "unknown kind: '{other}' (use runs|triages|reports|coverage|all)"
            ))
        }
    };
    recent_prefixed_dir_names(&root, prefix, limit)
}

pub(crate) fn collect_dashboard_snapshot(
    app_paths: &AppPaths,
) -> Result<DashboardSnapshot, String> {
    let artifact = artifact_contract(app_paths);
    let runs_root = artifact.runs_root;
    let triage_root = artifact.triage_root;
    let reports_root = artifact.reports_root;
    let coverage_root = artifact.coverage_root;
    let exports_root = artifact.exports_root;
    let mutated_root = artifact.mutated_root;
    let legacy_mutated_root = artifact.legacy_mutated_root;
    let metrics_path = artifact.metrics_root.join("latest.json");
    let seeds_onnx_count = count_seed_files(
        &default_seed_dir(app_paths, &TargetKind::Onnx),
        resolve_target_adapter(&TargetKind::Onnx).input_ext,
    )?;
    let seeds_gguf_count = count_seed_files(
        &default_seed_dir(app_paths, &TargetKind::Gguf),
        resolve_target_adapter(&TargetKind::Gguf).input_ext,
    )?;
    let seeds_safetensors_count = count_seed_files(
        &default_seed_dir(app_paths, &TargetKind::Safetensors),
        resolve_target_adapter(&TargetKind::Safetensors).input_ext,
    )?;
    let seeds_total_count = seeds_onnx_count + seeds_gguf_count + seeds_safetensors_count;

    let runs_count = count_prefixed_dirs(&runs_root, "run-")?;
    let triage_count = count_prefixed_dirs(&triage_root, "triage-")?;
    let report_count = count_prefixed_dirs(&reports_root, "report-")?;
    let coverage_count = count_prefixed_dirs(&coverage_root, "coverage-")?;

    let latest_run =
        latest_prefixed_dir_name(&runs_root, "run-")?.unwrap_or_else(|| "none".to_string());
    let latest_triage =
        latest_prefixed_dir_name(&triage_root, "triage-")?.unwrap_or_else(|| "none".to_string());
    let latest_report =
        latest_prefixed_dir_name(&reports_root, "report-")?.unwrap_or_else(|| "none".to_string());
    let latest_coverage = latest_prefixed_dir_name(&coverage_root, "coverage-")?
        .unwrap_or_else(|| "none".to_string());
    let recent_triage_ids = recent_prefixed_dir_names(&triage_root, "triage-", 8)?;
    let recent_report_ids = recent_prefixed_dir_names(&reports_root, "report-", 8)?;
    let recent_coverage_ids = recent_prefixed_dir_names(&coverage_root, "coverage-", 8)?;
    let latest_coverage_summary = if latest_coverage == "none" {
        "none".to_string()
    } else {
        coverage_root
            .join(&latest_coverage)
            .join("summary.json")
            .display()
            .to_string()
    };

    let latest_export = find_latest_export(&exports_root)?;
    let (
        latest_export_id,
        latest_export_path,
        latest_export_summary,
        latest_export_updated_at,
    ) = if let Some(view) = latest_export {
        (view.id, view.path, view.summary, view.updated_at)
    } else {
        (
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
        )
    };

    let latest_mutation = find_latest_mutation_manifest(&mutated_root, &legacy_mutated_root)?;
    let (
        latest_mutation_batch_id,
        latest_mutation_manifest_path,
        latest_mutation_target,
        latest_mutation_count,
        latest_mutation_updated_at,
        latest_mutation_source_corpus,
        latest_mutation_validation_summary,
    ) = if let Some(view) = latest_mutation {
        (
            view.batch_id,
            view.manifest_path,
            view.target,
            view.count,
            view.updated_at,
            view.source_corpus,
            view.validation_summary,
        )
    } else {
        (
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
        )
    };

    let latest_run_view = read_run_status(&runs_root, &latest_run)?;
    let (
        latest_run_target,
        latest_run_total,
        latest_run_success,
        latest_run_failed,
        latest_run_timeout,
    ) = if let Some(view) = latest_run_view {
        (view.target, view.total, view.success, view.failed, view.timeout)
    } else {
        (
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
        )
    };
    let latest_run_backend = "none".to_string();
    let run_state = if latest_run == "none" {
        "no_run".to_string()
    } else {
        "finished".to_string()
    };

    let latest_triage_summary = read_latest_triage_summary(&triage_root, &latest_triage)?;
    let (latest_triage_verdict, latest_triage_target) = match latest_triage_summary {
        Some(view) => (view.verdict, view.target),
        None => ("none".to_string(), "none".to_string()),
    };

    let latest_run_updated_at = dir_mtime_unix_string(&runs_root, &latest_run);
    let latest_triage_updated_at = dir_mtime_unix_string(&triage_root, &latest_triage);
    let latest_report_updated_at = dir_mtime_unix_string(&reports_root, &latest_report);

    let triage_verdicts = count_triage_verdicts(&triage_root)?;

    let recent_run_statuses = collect_recent_run_statuses(&runs_root, CHART_SERIES_LIMIT)?;
    let run_result_series = build_run_result_series(&recent_run_statuses);
    let throughput_proxy_series = build_throughput_proxy_series(&recent_run_statuses);
    let recent_triage_verdicts =
        collect_recent_triage_verdicts(&triage_root, CHART_SERIES_LIMIT)?;
    let crash_intake_series = build_crash_intake_series(&recent_triage_verdicts);
    let triage_verdict_breakdown = build_triage_verdict_breakdown(&triage_verdicts);

    let mut successful_runs_per_hour_proxy = "0".to_string();
    let mut new_crashes_per_hour = "0".to_string();
    let mut valid_crash_ratio = "not_available".to_string();
    let mut valid_crash_ratio_status = "not_available".to_string();
    let mut valid_crash_ratio_source = "not_available".to_string();
    let mut valid_crashes = "0".to_string();
    let mut total_crashes = "0".to_string();
    let mut triage_summary_count = "0".to_string();
    let mut global_error_rate_5m = "0.0".to_string();
    let metrics_exists = metrics_path.exists();
    if metrics_exists {
        let metrics = fs::read_to_string(&metrics_path)
            .map_err(|e| format!("failed to read '{}': {e}", metrics_path.display()))?;
        successful_runs_per_hour_proxy =
            extract_json_number_literal(&metrics, "successful_runs_per_hour_proxy")
                .or_else(|| extract_json_number_literal(&metrics, "new_paths_per_hour"))
                .unwrap_or_else(|| "0".to_string());
        new_crashes_per_hour = extract_json_number_literal(&metrics, "new_crashes_per_hour")
            .unwrap_or_else(|| "0".to_string());
        valid_crash_ratio_status =
            extract_json_string_literal(&metrics, "valid_crash_ratio_status")
                .unwrap_or_else(|| "legacy_unverified".to_string());
        valid_crash_ratio = if valid_crash_ratio_status == "available" {
            extract_json_number_literal(&metrics, "valid_crash_ratio")
                .unwrap_or_else(|| "not_available".to_string())
        } else {
            valid_crash_ratio_status.clone()
        };
        valid_crash_ratio_source =
            extract_json_string_literal(&metrics, "valid_crash_ratio_source")
                .unwrap_or_else(|| "legacy_event_log".to_string());
        valid_crashes = extract_json_number_literal(&metrics, "valid_crashes")
            .unwrap_or_else(|| "0".to_string());
        total_crashes = extract_json_number_literal(&metrics, "total_crashes")
            .unwrap_or_else(|| "0".to_string());
        triage_summary_count = extract_json_number_literal(&metrics, "triage_summary_count")
            .unwrap_or_else(|| "0".to_string());
        global_error_rate_5m = extract_json_number_literal(&metrics, "global_error_rate_5m")
            .unwrap_or_else(|| "0.0".to_string());
    }

    let latest_valid = find_latest_reproduced_triage(&triage_root)?;
    let (
        latest_valid_triage,
        latest_valid_input,
        latest_valid_signature,
        latest_valid_crash_kind,
        latest_valid_sanitizer,
        latest_valid_signal,
        latest_valid_normalized_frame_hash,
        latest_valid_signature_basis,
        latest_valid_crash_summary,
        latest_valid_summary,
        latest_valid_report,
        latest_valid_manifest,
        latest_valid_bundle,
        latest_suggested_severity,
        latest_severity_confidence,
        latest_suggested_cvss_vector,
    ) = if let Some(item) = latest_valid {
        let report_dir = find_report_dir_by_source_triage_id(&reports_root, &item.triage_id)?;
        let report = report_dir
            .as_ref()
            .map(|dir| dir.join("report.md").display().to_string())
            .unwrap_or_else(|| "none".to_string());
        let manifest = report_dir
            .as_ref()
            .map(|dir| dir.join("manifest.json").display().to_string())
            .filter(|path| Path::new(path).exists())
            .unwrap_or_else(|| "none".to_string());
        let bundle = report_dir
            .as_ref()
            .and_then(|dir| find_evidence_bundle_path(dir))
            .unwrap_or_else(|| "none".to_string());
        let severity = report_dir
            .as_ref()
            .and_then(|dir| read_report_severity_fields(dir).ok())
            .unwrap_or_default();
        (
            format!("triage-{}", item.triage_id),
            item.input,
            item.signature_top1,
            item.crash_kind,
            item.sanitizer,
            item.signal,
            item.normalized_frame_hash,
            item.signature_basis,
            item.crash_summary,
            item.summary_path,
            report,
            manifest,
            bundle,
            severity.suggested_severity,
            severity.confidence,
            severity.suggested_cvss_vector,
        )
    } else {
        (
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
            "none".to_string(),
        )
    };

    Ok(DashboardSnapshot {
        generated_at: now_unix(),
        data_dir: app_paths.data_dir.display().to_string(),
        seeds_dir: app_paths.seeds_dir.display().to_string(),
        runs_count,
        triage_count,
        report_count,
        latest_run,
        latest_triage,
        latest_report,
        metrics_exists,
        successful_runs_per_hour_proxy,
        new_crashes_per_hour,
        valid_crash_ratio,
        valid_crash_ratio_status,
        valid_crash_ratio_source,
        valid_crashes,
        total_crashes,
        triage_summary_count,
        global_error_rate_5m,
        latest_valid_triage,
        latest_valid_input,
        latest_valid_signature,
        latest_valid_crash_kind,
        latest_valid_sanitizer,
        latest_valid_signal,
        latest_valid_normalized_frame_hash,
        latest_valid_signature_basis,
        latest_valid_crash_summary,
        latest_valid_summary,
        latest_valid_report,
        latest_valid_manifest,
        latest_valid_bundle,
        latest_suggested_severity,
        latest_severity_confidence,
        latest_suggested_cvss_vector,
        recent_triage_ids,
        recent_report_ids,
        recent_coverage_ids,
        seeds_onnx_count,
        seeds_gguf_count,
        seeds_safetensors_count,
        seeds_total_count,
        coverage_count,
        latest_coverage,
        latest_coverage_summary,
        latest_export_id,
        latest_export_path,
        latest_export_summary,
        latest_mutation_batch_id,
        latest_mutation_manifest_path,
        latest_mutation_target,
        latest_mutation_count,
        latest_run_target,
        latest_run_backend,
        latest_run_total,
        latest_run_success,
        latest_run_failed,
        latest_run_timeout,
        triage_verdict_reproduced: triage_verdicts.reproduced,
        triage_verdict_manual_review: triage_verdicts.manual_review,
        triage_verdict_not_reproduced: triage_verdicts.not_reproduced,
        triage_verdict_timeout: triage_verdicts.timeout,
        triage_verdict_infra_oom: triage_verdicts.infra_oom,
        triage_verdict_flaky: triage_verdicts.flaky,
        triage_verdict_other: triage_verdicts.other,
        run_state,
        latest_triage_verdict,
        latest_triage_target,
        latest_run_updated_at,
        latest_triage_updated_at,
        latest_report_updated_at,
        latest_export_updated_at,
        latest_mutation_updated_at,
        latest_mutation_source_corpus,
        latest_mutation_validation_summary,
        run_result_series,
        throughput_proxy_series,
        crash_intake_series,
        triage_verdict_breakdown,
    })
}

fn count_seed_files(root: &Path, ext: &str) -> Result<usize, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in
        fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if path.is_file() && has_ext(&path, ext) {
            count += 1;
        }
    }
    Ok(count)
}

fn find_latest_reproduced_triage(
    triage_root: &Path,
) -> Result<Option<ReproducedTriageView>, String> {
    if !triage_root.exists() {
        return Ok(None);
    }

    let mut latest: Option<(u128, ReproducedTriageView)> = None;
    for entry in fs::read_dir(triage_root)
        .map_err(|e| format!("failed to read '{}': {e}", triage_root.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read triage entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id_text) = name.strip_prefix("triage-") else {
            continue;
        };
        let Ok(id) = id_text.parse::<u128>() else {
            continue;
        };

        let summary_path = path.join("summary.json");
        if !summary_path.exists() {
            continue;
        }
        let summary = fs::read_to_string(&summary_path)
            .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;
        let verdict = extract_json_string_literal(&summary, "verdict").unwrap_or_default();
        if verdict != "reproduced" {
            continue;
        }
        let input =
            extract_json_string_literal(&summary, "input").unwrap_or_else(|| "unknown".to_string());
        let signature_top1 =
            extract_first_signature_top1(&summary).unwrap_or_else(|| "none".to_string());
        let crash_kind = extract_json_string_literal(&summary, "crash_kind")
            .unwrap_or_else(|| "unknown".to_string());
        let sanitizer = extract_json_string_literal(&summary, "sanitizer")
            .unwrap_or_else(|| "unknown".to_string());
        let signal = extract_json_string_literal(&summary, "signal")
            .unwrap_or_else(|| "unknown".to_string());
        let normalized_frame_hash = extract_json_string_literal(&summary, "normalized_frame_hash")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "legacy_signature_top3".to_string());
        let signature_basis = extract_json_string_literal(&summary, "signature_basis")
            .unwrap_or_else(|| "signature_top3".to_string());
        let crash_summary = extract_json_string_literal(&summary, "crash_summary")
            .unwrap_or_else(|| "not_available".to_string());

        let item = ReproducedTriageView {
            triage_id: id_text.to_string(),
            input,
            signature_top1,
            crash_kind,
            sanitizer,
            signal,
            normalized_frame_hash,
            signature_basis,
            crash_summary,
            summary_path: summary_path.display().to_string(),
        };
        match &latest {
            Some((best, _)) if id <= *best => {}
            _ => latest = Some((id, item)),
        }
    }
    Ok(latest.map(|(_, item)| item))
}

fn find_report_dir_by_source_triage_id(
    reports_root: &Path,
    triage_id: &str,
) -> Result<Option<PathBuf>, String> {
    if !reports_root.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(reports_root)
        .map_err(|e| format!("failed to read '{}': {e}", reports_root.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read report entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        let meta = fs::read_to_string(&meta_path)
            .map_err(|e| format!("failed to read '{}': {e}", meta_path.display()))?;
        let source_triage =
            extract_json_string_literal(&meta, "source_triage_id").unwrap_or_default();
        if source_triage != triage_id {
            continue;
        }
        return Ok(Some(path));
    }
    Ok(None)
}

fn find_evidence_bundle_path(report_dir: &Path) -> Option<String> {
    let mut selected: Option<String> = None;
    let entries = fs::read_dir(report_dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with("-evidence.zip") {
            selected = Some(path.display().to_string());
        }
    }
    selected
}

fn read_report_severity_fields(report_dir: &Path) -> Result<ReportSeverityView, String> {
    let meta_path = report_dir.join("meta.json");
    let meta = fs::read_to_string(&meta_path)
        .map_err(|e| format!("failed to read '{}': {e}", meta_path.display()))?;
    Ok(ReportSeverityView {
        suggested_severity: extract_json_string_literal(&meta, "suggested_severity")
            .unwrap_or_else(|| "none".to_string()),
        confidence: extract_json_string_literal(&meta, "severity_confidence")
            .unwrap_or_else(|| "none".to_string()),
        suggested_cvss_vector: extract_json_string_literal(&meta, "suggested_cvss_vector")
            .unwrap_or_else(|| "none".to_string()),
    })
}

fn count_prefixed_dirs(root: &Path, prefix: &str) -> Result<usize, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in
        fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(prefix) {
            count += 1;
        }
    }
    Ok(count)
}

fn latest_prefixed_dir_name(root: &Path, prefix: &str) -> Result<Option<String>, String> {
    if !root.exists() {
        return Ok(None);
    }
    let mut latest: Option<(u128, String)> = None;
    for entry in
        fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id_text) = name.strip_prefix(prefix) else {
            continue;
        };
        let Ok(id) = id_text.parse::<u128>() else {
            continue;
        };
        match &latest {
            Some((best, _)) if id <= *best => {}
            _ => latest = Some((id, name.to_string())),
        }
    }
    Ok(latest.map(|(_, n)| n))
}

pub(crate) fn recent_prefixed_dir_names(
    root: &Path,
    prefix: &str,
    limit: usize,
) -> Result<Vec<String>, String> {
    if !root.exists() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut rows: Vec<(u128, String)> = Vec::new();
    for entry in
        fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id_text) = name.strip_prefix(prefix) else {
            continue;
        };
        let Ok(id) = id_text.parse::<u128>() else {
            continue;
        };
        rows.push((id, name.to_string()));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(rows.into_iter().take(limit).map(|(_, name)| name).collect())
}

struct LatestExportView {
    id: String,
    path: String,
    summary: String,
    updated_at: String,
}

struct LatestMutationView {
    batch_id: String,
    manifest_path: String,
    target: String,
    count: String,
    updated_at: String,
    source_corpus: String,
    validation_summary: String,
}

fn find_latest_export(exports_root: &Path) -> Result<Option<LatestExportView>, String> {
    if !exports_root.exists() {
        return Ok(None);
    }
    let mut best: Option<(SystemTime, LatestExportView)> = None;
    for entry in fs::read_dir(exports_root)
        .map_err(|e| format!("failed to read '{}': {e}", exports_root.display()))?
    {
        let entry = entry
            .map_err(|e| format!("failed to read entry in '{}': {e}", exports_root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let mtime = entry_mtime(&path).unwrap_or(SystemTime::UNIX_EPOCH);
        let manifest_path = path.join("manifest.json");
        let summary = if manifest_path.is_file() {
            manifest_path.display().to_string()
        } else {
            "no_manifest".to_string()
        };
        let view = LatestExportView {
            id: name.to_string(),
            path: path.display().to_string(),
            summary,
            updated_at: system_time_to_unix_string(mtime),
        };
        match &best {
            Some((best_mtime, _)) if mtime <= *best_mtime => {}
            _ => best = Some((mtime, view)),
        }
    }
    Ok(best.map(|(_, v)| v))
}

fn find_latest_mutation_manifest(
    primary_root: &Path,
    legacy_root: &Path,
) -> Result<Option<LatestMutationView>, String> {
    if let Some(view) = scan_mutation_tree(primary_root)? {
        return Ok(Some(view));
    }
    scan_mutation_tree(legacy_root)
}

fn scan_mutation_tree(root: &Path) -> Result<Option<LatestMutationView>, String> {
    if !root.exists() {
        return Ok(None);
    }
    let mut best: Option<(SystemTime, LatestMutationView)> = None;
    for top_entry in
        fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
    {
        let top_entry = top_entry
            .map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let top_path = top_entry.path();
        if !top_path.is_dir() {
            continue;
        }
        let top_name = top_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let manifest_at_top = top_path.join("manifest.json");
        if manifest_at_top.is_file() {
            consider_mutation_manifest(&manifest_at_top, &top_path, &top_name, None, &mut best)?;
            continue;
        }
        for sub_entry in fs::read_dir(&top_path)
            .map_err(|e| format!("failed to read '{}': {e}", top_path.display()))?
        {
            let sub_entry = sub_entry
                .map_err(|e| format!("failed to read entry in '{}': {e}", top_path.display()))?;
            let sub_path = sub_entry.path();
            if !sub_path.is_dir() {
                continue;
            }
            let manifest = sub_path.join("manifest.json");
            if !manifest.is_file() {
                continue;
            }
            let sub_name = sub_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            consider_mutation_manifest(
                &manifest,
                &sub_path,
                &sub_name,
                Some(top_name.clone()),
                &mut best,
            )?;
        }
    }
    Ok(best.map(|(_, v)| v))
}

fn consider_mutation_manifest(
    manifest_path: &Path,
    batch_dir: &Path,
    batch_name: &str,
    parent_target_hint: Option<String>,
    best: &mut Option<(SystemTime, LatestMutationView)>,
) -> Result<(), String> {
    let mtime = entry_mtime(manifest_path)
        .or_else(|| entry_mtime(batch_dir))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let body = fs::read_to_string(manifest_path)
        .map_err(|e| format!("failed to read '{}': {e}", manifest_path.display()))?;
    let target = extract_json_string_literal(&body, "target")
        .or(parent_target_hint)
        .unwrap_or_else(|| "unknown".to_string());
    let count = extract_json_number_literal(&body, "generated")
        .or_else(|| extract_json_number_literal(&body, "requested"))
        .unwrap_or_else(|| "none".to_string());
    let source_corpus = extract_json_string_literal(&body, "input_dir")
        .unwrap_or_else(|| "none".to_string());
    let validation_summary = extract_json_string_literal(&body, "validation_status")
        .or_else(|| extract_json_string_literal(&body, "validation_summary"))
        .unwrap_or_else(|| "none".to_string());
    let view = LatestMutationView {
        batch_id: batch_name.to_string(),
        manifest_path: manifest_path.display().to_string(),
        target,
        count,
        updated_at: system_time_to_unix_string(mtime),
        source_corpus,
        validation_summary,
    };
    match best {
        Some((best_mtime, _)) if mtime <= *best_mtime => {}
        _ => *best = Some((mtime, view)),
    }
    Ok(())
}

fn entry_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn system_time_to_unix_string(t: SystemTime) -> String {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "none".to_string())
}

fn dir_mtime_unix_string(root: &Path, dir_name: &str) -> String {
    if dir_name == "none" {
        return "none".to_string();
    }
    let path = root.join(dir_name);
    entry_mtime(&path)
        .map(system_time_to_unix_string)
        .unwrap_or_else(|| "none".to_string())
}

struct LatestTriageSummaryView {
    verdict: String,
    target: String,
}

fn read_latest_triage_summary(
    triage_root: &Path,
    triage_dir_name: &str,
) -> Result<Option<LatestTriageSummaryView>, String> {
    if triage_dir_name == "none" {
        return Ok(None);
    }
    let summary_path = triage_root.join(triage_dir_name).join("summary.json");
    if !summary_path.is_file() {
        return Ok(None);
    }
    let body = fs::read_to_string(&summary_path)
        .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;
    Ok(Some(LatestTriageSummaryView {
        verdict: extract_json_string_literal(&body, "verdict")
            .unwrap_or_else(|| "none".to_string()),
        target: extract_json_string_literal(&body, "target")
            .unwrap_or_else(|| "none".to_string()),
    }))
}

pub(crate) struct RunStatusView {
    pub(crate) target: String,
    pub(crate) total: String,
    pub(crate) success: String,
    pub(crate) failed: String,
    pub(crate) timeout: String,
}

pub(crate) fn read_run_status(
    runs_root: &Path,
    run_dir_name: &str,
) -> Result<Option<RunStatusView>, String> {
    if run_dir_name == "none" {
        return Ok(None);
    }
    let status_path = runs_root.join(run_dir_name).join("status.json");
    if !status_path.is_file() {
        return Ok(None);
    }
    let body = fs::read_to_string(&status_path)
        .map_err(|e| format!("failed to read '{}': {e}", status_path.display()))?;
    Ok(Some(RunStatusView {
        target: extract_json_string_literal(&body, "target")
            .unwrap_or_else(|| "none".to_string()),
        total: extract_json_number_literal(&body, "total")
            .unwrap_or_else(|| "none".to_string()),
        success: extract_json_number_literal(&body, "success")
            .unwrap_or_else(|| "none".to_string()),
        failed: extract_json_number_literal(&body, "failed")
            .unwrap_or_else(|| "none".to_string()),
        timeout: extract_json_number_literal(&body, "timeout")
            .unwrap_or_else(|| "none".to_string()),
    }))
}

#[derive(Default)]
pub(crate) struct TriageVerdictCounts {
    pub(crate) reproduced: usize,
    pub(crate) manual_review: usize,
    pub(crate) not_reproduced: usize,
    pub(crate) timeout: usize,
    pub(crate) infra_oom: usize,
    pub(crate) flaky: usize,
    pub(crate) other: usize,
}

fn count_triage_verdicts(triage_root: &Path) -> Result<TriageVerdictCounts, String> {
    let mut counts = TriageVerdictCounts::default();
    if !triage_root.exists() {
        return Ok(counts);
    }
    for entry in fs::read_dir(triage_root)
        .map_err(|e| format!("failed to read '{}': {e}", triage_root.display()))?
    {
        let entry = entry
            .map_err(|e| format!("failed to read triage verdict entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("triage-") {
            continue;
        }
        let summary_path = path.join("summary.json");
        if !summary_path.is_file() {
            continue;
        }
        let body = fs::read_to_string(&summary_path)
            .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;
        let verdict = extract_json_string_literal(&body, "verdict").unwrap_or_default();
        match verdict.as_str() {
            "reproduced" => counts.reproduced += 1,
            "manual_review" => counts.manual_review += 1,
            "not_reproduced" => counts.not_reproduced += 1,
            "timeout" => counts.timeout += 1,
            "infra_oom" => counts.infra_oom += 1,
            "flaky" => counts.flaky += 1,
            _ => counts.other += 1,
        }
    }
    Ok(counts)
}
