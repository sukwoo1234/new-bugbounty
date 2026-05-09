use std::{
    fs,
    path::Path,
};

use crate::common::{AppPaths, artifact_contract, has_ext, now_unix};
use crate::json_utils::{
    extract_first_signature_top1, extract_json_number_literal, extract_json_string_literal,
};
use crate::target::{TargetKind, default_seed_dir, resolve_target_adapter};

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
    pub(crate) global_error_rate_5m: String,
    pub(crate) latest_valid_triage: String,
    pub(crate) latest_valid_input: String,
    pub(crate) latest_valid_signature: String,
    pub(crate) latest_valid_summary: String,
    pub(crate) latest_valid_report: String,
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
}

struct ReproducedTriageView {
    triage_id: String,
    input: String,
    signature_top1: String,
    summary_path: String,
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
        other => return Err(format!("unknown kind: '{other}' (use runs|triages|reports|coverage|all)")),
    };
    recent_prefixed_dir_names(&root, prefix, limit)
}

pub(crate) fn collect_dashboard_snapshot(app_paths: &AppPaths) -> Result<DashboardSnapshot, String> {
    let artifact = artifact_contract(app_paths);
    let runs_root = artifact.runs_root;
    let triage_root = artifact.triage_root;
    let reports_root = artifact.reports_root;
    let coverage_root = artifact.coverage_root;
    let metrics_path = artifact.metrics_root.join("latest.json");
    let seeds_onnx_count = count_seed_files(&default_seed_dir(app_paths, &TargetKind::Onnx), resolve_target_adapter(&TargetKind::Onnx).input_ext)?;
    let seeds_gguf_count = count_seed_files(&default_seed_dir(app_paths, &TargetKind::Gguf), resolve_target_adapter(&TargetKind::Gguf).input_ext)?;
    let seeds_safetensors_count = count_seed_files(&default_seed_dir(app_paths, &TargetKind::Safetensors), resolve_target_adapter(&TargetKind::Safetensors).input_ext)?;
    let seeds_total_count = seeds_onnx_count + seeds_gguf_count + seeds_safetensors_count;

    let runs_count = count_prefixed_dirs(&runs_root, "run-")?;
    let triage_count = count_prefixed_dirs(&triage_root, "triage-")?;
    let report_count = count_prefixed_dirs(&reports_root, "report-")?;
    let coverage_count = count_prefixed_dirs(&coverage_root, "coverage-")?;

    let latest_run = latest_prefixed_dir_name(&runs_root, "run-")?.unwrap_or_else(|| "none".to_string());
    let latest_triage =
        latest_prefixed_dir_name(&triage_root, "triage-")?.unwrap_or_else(|| "none".to_string());
    let latest_report =
        latest_prefixed_dir_name(&reports_root, "report-")?.unwrap_or_else(|| "none".to_string());
    let latest_coverage =
        latest_prefixed_dir_name(&coverage_root, "coverage-")?.unwrap_or_else(|| "none".to_string());
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

    let mut successful_runs_per_hour_proxy = "0".to_string();
    let mut new_crashes_per_hour = "0".to_string();
    let mut valid_crash_ratio = "not_available".to_string();
    let mut valid_crash_ratio_status = "not_available".to_string();
    let mut global_error_rate_5m = "0.0".to_string();
    let metrics_exists = metrics_path.exists();
    if metrics_exists {
        let metrics = fs::read_to_string(&metrics_path)
            .map_err(|e| format!("failed to read '{}': {e}", metrics_path.display()))?;
        successful_runs_per_hour_proxy =
            extract_json_number_literal(&metrics, "successful_runs_per_hour_proxy")
                .or_else(|| extract_json_number_literal(&metrics, "new_paths_per_hour"))
                .unwrap_or_else(|| "0".to_string());
        new_crashes_per_hour =
            extract_json_number_literal(&metrics, "new_crashes_per_hour").unwrap_or_else(|| "0".to_string());
        valid_crash_ratio_status = extract_json_string_literal(&metrics, "valid_crash_ratio_status")
            .unwrap_or_else(|| "legacy_unverified".to_string());
        valid_crash_ratio = if valid_crash_ratio_status == "available" {
            extract_json_number_literal(&metrics, "valid_crash_ratio")
                .unwrap_or_else(|| "not_available".to_string())
        } else {
            valid_crash_ratio_status.clone()
        };
        global_error_rate_5m =
            extract_json_number_literal(&metrics, "global_error_rate_5m").unwrap_or_else(|| "0.0".to_string());
    }

    let latest_valid = find_latest_reproduced_triage(&triage_root)?;
    let (latest_valid_triage, latest_valid_input, latest_valid_signature, latest_valid_summary, latest_valid_report) =
        if let Some(item) = latest_valid {
            let report =
                find_report_by_source_triage_id(&reports_root, &item.triage_id)?.unwrap_or_else(|| "none".to_string());
            (
                format!("triage-{}", item.triage_id),
                item.input,
                item.signature_top1,
                item.summary_path,
                report,
            )
        } else {
            (
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
        global_error_rate_5m,
        latest_valid_triage,
        latest_valid_input,
        latest_valid_signature,
        latest_valid_summary,
        latest_valid_report,
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
    })
}

fn count_seed_files(root: &Path, ext: &str) -> Result<usize, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if path.is_file() && has_ext(&path, ext) {
            count += 1;
        }
    }
    Ok(count)
}

fn find_latest_reproduced_triage(triage_root: &Path) -> Result<Option<ReproducedTriageView>, String> {
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
        let input = extract_json_string_literal(&summary, "input").unwrap_or_else(|| "unknown".to_string());
        let signature_top1 =
            extract_first_signature_top1(&summary).unwrap_or_else(|| "none".to_string());

        let item = ReproducedTriageView {
            triage_id: id_text.to_string(),
            input,
            signature_top1,
            summary_path: summary_path.display().to_string(),
        };
        match &latest {
            Some((best, _)) if id <= *best => {}
            _ => latest = Some((id, item)),
        }
    }
    Ok(latest.map(|(_, item)| item))
}

fn find_report_by_source_triage_id(reports_root: &Path, triage_id: &str) -> Result<Option<String>, String> {
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
        let source_triage = extract_json_string_literal(&meta, "source_triage_id").unwrap_or_default();
        if source_triage != triage_id {
            continue;
        }
        let report_path = path.join("report.md");
        if report_path.exists() {
            return Ok(Some(report_path.display().to_string()));
        }
        return Ok(Some(path.display().to_string()));
    }
    Ok(None)
}

fn count_prefixed_dirs(root: &Path, prefix: &str) -> Result<usize, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
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
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
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

fn recent_prefixed_dir_names(root: &Path, prefix: &str, limit: usize) -> Result<Vec<String>, String> {
    if !root.exists() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut rows: Vec<(u128, String)> = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
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
