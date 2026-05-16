use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::dashboard_data::entry_mtime;
use crate::json_utils::{
    extract_first_signature_top1, extract_json_number_literal, extract_json_string_literal,
};

pub(crate) struct ReproducedTriageView {
    pub(crate) triage_id: String,
    pub(crate) input: String,
    pub(crate) signature_top1: String,
    pub(crate) crash_kind: String,
    pub(crate) sanitizer: String,
    pub(crate) signal: String,
    pub(crate) normalized_frame_hash: String,
    pub(crate) signature_basis: String,
    pub(crate) crash_summary: String,
    pub(crate) summary_path: String,
}

#[derive(Default)]
pub(crate) struct ReportSeverityView {
    pub(crate) suggested_severity: String,
    pub(crate) confidence: String,
    pub(crate) suggested_cvss_vector: String,
}

pub(crate) struct LatestExportView {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) summary: String,
    pub(crate) updated_at: String,
}

pub(crate) struct LatestMutationView {
    pub(crate) batch_id: String,
    pub(crate) manifest_path: String,
    pub(crate) target: String,
    pub(crate) count: String,
    pub(crate) updated_at: String,
    pub(crate) source_corpus: String,
    pub(crate) validation_summary: String,
}

pub(crate) fn find_latest_reproduced_triage(
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
            extract_first_signature_top1(&summary).unwrap_or_else(|| "not_available".to_string());
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

pub(crate) fn find_report_dir_by_source_triage_id(
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

pub(crate) fn find_evidence_bundle_path(report_dir: &Path) -> Option<String> {
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

pub(crate) fn read_report_severity_fields(report_dir: &Path) -> Result<ReportSeverityView, String> {
    let meta_path = report_dir.join("meta.json");
    let meta = fs::read_to_string(&meta_path)
        .map_err(|e| format!("failed to read '{}': {e}", meta_path.display()))?;
    Ok(ReportSeverityView {
        suggested_severity: extract_json_string_literal(&meta, "suggested_severity")
            .unwrap_or_else(|| "not_available".to_string()),
        confidence: extract_json_string_literal(&meta, "severity_confidence")
            .unwrap_or_else(|| "not_available".to_string()),
        suggested_cvss_vector: extract_json_string_literal(&meta, "suggested_cvss_vector")
            .unwrap_or_else(|| "not_available".to_string()),
    })
}

pub(crate) fn count_prefixed_dirs(root: &Path, prefix: &str) -> Result<usize, String> {
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

pub(crate) fn latest_prefixed_dir_name(
    root: &Path,
    prefix: &str,
) -> Result<Option<String>, String> {
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

pub(crate) fn find_latest_export(
    exports_root: &Path,
) -> Result<Option<LatestExportView>, String> {
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

pub(crate) fn find_latest_mutation_manifest(
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
        .unwrap_or_else(|| "not_available".to_string());
    let source_corpus = extract_json_string_literal(&body, "source_path")
        .or_else(|| extract_json_string_literal(&body, "input_dir"))
        .unwrap_or_else(|| "not_available".to_string());
    let validation_summary = extract_json_string_literal(&body, "validation_status")
        .or_else(|| extract_json_string_literal(&body, "validation_summary"))
        .unwrap_or_else(|| "not_available".to_string());
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

pub(crate) fn system_time_to_unix_string(t: SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "not_available".to_string())
}
