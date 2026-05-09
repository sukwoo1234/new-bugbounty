use std::{fs, fs::OpenOptions, io::Write, path::Path};

use crate::common::{AppPaths, now_unix};
use crate::json_utils::{extract_json_string_literal, extract_json_u64_field};

pub(crate) struct MetricEvent {
    pub(crate) ts: u64,
    pub(crate) kind: &'static str,
    pub(crate) total: u64,
    pub(crate) errors: u64,
    pub(crate) successful_runs_proxy: u64,
    pub(crate) new_crashes: u64,
    pub(crate) valid_crashes: u64,
    pub(crate) total_crashes: u64,
}

pub(crate) fn record_metrics_event(app_paths: &AppPaths, event: MetricEvent) -> Result<(), String> {
    let metrics_dir = app_paths.data_dir.join("metrics");
    fs::create_dir_all(&metrics_dir)
        .map_err(|e| format!("failed to create metrics dir '{}': {e}", metrics_dir.display()))?;

    let events_path = metrics_dir.join("events.jsonl");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)
        .map_err(|e| format!("failed to open '{}': {e}", events_path.display()))?;

    let line = format!(
        "{{\"ts\":{},\"kind\":\"{}\",\"total\":{},\"errors\":{},\"successful_runs_proxy\":{},\"new_crashes\":{},\"valid_crashes\":{},\"total_crashes\":{}}}\n",
        event.ts,
        event.kind,
        event.total,
        event.errors,
        event.successful_runs_proxy,
        event.new_crashes,
        event.valid_crashes,
        event.total_crashes
    );
    f.write_all(line.as_bytes())
        .map_err(|e| format!("failed to append '{}': {e}", events_path.display()))?;

    let snapshot = build_metrics_snapshot(app_paths, &events_path, now_unix())?;
    let snapshot_path = metrics_dir.join("latest.json");
    // atomic write: temp 파일에 먼저 쓰고 rename으로 교체해서 partial 파일 상태 방지
    let tmp_path = metrics_dir.join("latest.json.tmp");
    fs::write(&tmp_path, snapshot)
        .map_err(|e| format!("failed to write temp '{}': {e}", tmp_path.display()))?;
    fs::rename(&tmp_path, &snapshot_path).map_err(|e| {
        format!(
            "failed to rename '{}' -> '{}': {e}",
            tmp_path.display(),
            snapshot_path.display()
        )
    })?;
    Ok(())
}

fn build_metrics_snapshot(app_paths: &AppPaths, events_path: &Path, now_ts: u64) -> Result<String, String> {
    let content = fs::read_to_string(events_path)
        .map_err(|e| format!("failed to read '{}': {e}", events_path.display()))?;
    let mut successful_runs_proxy_1h = 0u64;
    let mut new_crashes_1h = 0u64;
    let mut total_5m = 0u64;
    let mut errors_5m = 0u64;

    for line in content.lines() {
        let ts = extract_json_u64_field(line, "ts").unwrap_or(0);
        let total = extract_json_u64_field(line, "total").unwrap_or(0);
        let errors = extract_json_u64_field(line, "errors").unwrap_or(0);
        let successful_runs_proxy = extract_json_u64_field(line, "successful_runs_proxy")
            .or_else(|| extract_json_u64_field(line, "new_paths"))
            .unwrap_or(0);
        let new_crashes = extract_json_u64_field(line, "new_crashes").unwrap_or(0);
        if now_ts.saturating_sub(ts) <= 3600 {
            successful_runs_proxy_1h += successful_runs_proxy;
            new_crashes_1h += new_crashes;
        }
        if now_ts.saturating_sub(ts) <= 300 {
            total_5m += total;
            errors_5m += errors;
        }
    }

    let triage_ratio = calculate_valid_crash_ratio_from_triage(&app_paths.data_dir.join("triage"))?;
    let (valid_ratio_literal, valid_ratio_status) = if triage_ratio.total_crashes == 0 {
        ("null".to_string(), "not_available")
    } else {
        (
            format!(
                "{:.4}",
                triage_ratio.valid_crashes as f64 / triage_ratio.total_crashes as f64
            ),
            "available",
        )
    };
    let error_rate_5m = if total_5m == 0 {
        0.0
    } else {
        errors_5m as f64 / total_5m as f64
    };

    Ok(format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"generated_at\": {},\n  \"metrics\": {{\n    \"successful_runs_per_hour_proxy\": {},\n    \"new_crashes_per_hour\": {},\n    \"valid_crash_ratio\": {},\n    \"valid_crash_ratio_status\": \"{}\",\n    \"valid_crash_ratio_source\": \"triage_summary_scan\",\n    \"valid_crashes\": {},\n    \"total_crashes\": {},\n    \"triage_summary_count\": {},\n    \"global_error_rate_5m\": {:.4}\n  }}\n}}\n",
        now_ts,
        successful_runs_proxy_1h,
        new_crashes_1h,
        valid_ratio_literal,
        valid_ratio_status,
        triage_ratio.valid_crashes,
        triage_ratio.total_crashes,
        triage_ratio.summary_count,
        error_rate_5m
    ))
}

struct TriageCrashRatio {
    valid_crashes: u64,
    total_crashes: u64,
    summary_count: u64,
}

fn calculate_valid_crash_ratio_from_triage(triage_root: &Path) -> Result<TriageCrashRatio, String> {
    let mut ratio = TriageCrashRatio {
        valid_crashes: 0,
        total_crashes: 0,
        summary_count: 0,
    };

    if !triage_root.exists() {
        return Ok(ratio);
    }

    for entry in fs::read_dir(triage_root)
        .map_err(|e| format!("failed to read triage dir '{}': {e}", triage_root.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read triage entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let summary_path = path.join("summary.json");
        if !summary_path.is_file() {
            continue;
        }

        let summary = fs::read_to_string(&summary_path)
            .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;
        ratio.summary_count += 1;

        let crashed_count = extract_json_u64_field(&summary, "crashed_count").unwrap_or(0);
        if crashed_count == 0 {
            continue;
        }

        ratio.total_crashes += 1;
        if extract_json_string_literal(&summary, "verdict").as_deref() == Some("reproduced") {
            ratio.valid_crashes += 1;
        }
    }

    Ok(ratio)
}
