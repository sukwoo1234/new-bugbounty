use std::{fs, path::{Path, PathBuf}, time::{Duration, SystemTime}};

use crate::common::{AppPaths, command_exists, command_with_core_dump_off};

pub(crate) struct RetentionStats {
    pub(crate) compressed_logs: usize,
    pub(crate) deleted_dirs: usize,
    pub(crate) skipped_log_compress: usize,
}

pub(crate) fn apply_retention_policy(
    app_paths: &AppPaths,
    retention_days: u64,
) -> Result<RetentionStats, String> {
    let cutoff_secs = retention_days
        .checked_mul(24 * 60 * 60)
        .ok_or_else(|| "retention days overflow".to_string())?;
    let now = SystemTime::now();
    let mut stats = RetentionStats {
        compressed_logs: 0,
        deleted_dirs: 0,
        skipped_log_compress: 0,
    };

    let logs = collect_log_files(&app_paths.data_dir)?;
    let has_zstd = command_exists("zstd");
    for log in logs {
        if !is_older_than(&log, now, cutoff_secs)? {
            continue;
        }
        if !has_zstd {
            stats.skipped_log_compress += 1;
            continue;
        }
        let status = command_with_core_dump_off("zstd")
            .args(["-q", "-f", "--rm", &log.display().to_string()])
            .status()
            .map_err(|e| format!("failed to execute zstd for '{}': {e}", log.display()))?;
        if status.success() {
            stats.compressed_logs += 1;
        }
    }

    for (base, prefix) in [("runs", "run-"), ("triage", "triage-"), ("reports", "report-")] {
        let root = app_paths.data_dir.join(base);
        if !root.exists() {
            continue;
        }
        for entry in
            fs::read_dir(&root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
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
            if !name.starts_with(prefix) {
                continue;
            }
            if is_older_than(&path, now, cutoff_secs)? {
                fs::remove_dir_all(&path)
                    .map_err(|e| format!("failed to remove old dir '{}': {e}", path.display()))?;
                stats.deleted_dirs += 1;
            }
        }
    }

    Ok(stats)
}

fn collect_log_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    collect_log_files_recursive(root, &mut out)?;
    Ok(out)
}

fn collect_log_files_recursive(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| format!("failed to read '{}': {e}", root.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read entry in '{}': {e}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_log_files_recursive(&path, out)?;
            continue;
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("log"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}

fn is_older_than(path: &Path, now: SystemTime, age_secs: u64) -> Result<bool, String> {
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| format!("failed to read mtime '{}': {e}", path.display()))?;
    let elapsed = now
        .duration_since(modified)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();
    Ok(elapsed > age_secs)
}

