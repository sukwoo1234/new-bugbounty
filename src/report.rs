use std::{fs, path::{Path, PathBuf}};

use crate::{json_escape, now_unix, sha256_file, AppPaths};
use crate::retention::apply_retention_policy;

pub(crate) fn run_report_pipeline(app_paths: &AppPaths) -> Result<(), String> {
    let retention = apply_retention_policy(app_paths, 30)?;
    let (triage_id, triage_dir, summary_path) = find_latest_triage_summary(&app_paths.data_dir)?;
    let summary = fs::read_to_string(&summary_path)
        .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;

    let target =
        extract_json_string_field(&summary, "target").unwrap_or_else(|| "unknown".to_string());
    let input =
        extract_json_string_field(&summary, "input").unwrap_or_else(|| "unknown".to_string());
    let verdict =
        extract_json_string_field(&summary, "verdict").unwrap_or_else(|| "unknown".to_string());
    let repro_retries = extract_json_u64_field(&summary, "repro_retries").unwrap_or(3);
    let timeout_sec = extract_json_u64_field(&summary, "timeout_sec").unwrap_or(60);
    let signature_top3 = extract_first_signature_top3(&summary);

    let input_sha256 = if input != "unknown" {
        sha256_file(Path::new(&input)).unwrap_or_else(|_| "unavailable".to_string())
    } else {
        "unavailable".to_string()
    };

    let report_id = now_unix();
    let report_dir = app_paths
        .data_dir
        .join("reports")
        .join(format!("report-{report_id}"));
    fs::create_dir_all(&report_dir)
        .map_err(|e| format!("failed to create report dir '{}': {e}", report_dir.display()))?;

    let crash_report = build_crash_report(&triage_dir, &summary);
    let crash_report_path = report_dir.join("crash_report.txt");
    fs::write(&crash_report_path, crash_report)
        .map_err(|e| format!("failed to write '{}': {e}", crash_report_path.display()))?;

    let repro_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\ntool triage --target {} --input '{}' --repro-retries {} --timeout-sec {}\n",
        target,
        shell_escape_single_quoted(&input),
        repro_retries,
        timeout_sec
    );
    let repro_path = report_dir.join("repro.sh");
    fs::write(&repro_path, repro_script)
        .map_err(|e| format!("failed to write '{}': {e}", repro_path.display()))?;

    let meta_json = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"report_id\": \"{}\",\n  \"source_triage_id\": \"{}\",\n  \"source_summary\": \"{}\",\n  \"target\": \"{}\",\n  \"input\": \"{}\",\n  \"input_sha256\": \"{}\",\n  \"verdict\": \"{}\",\n  \"retention_days\": 30,\n  \"retention\": {{\n    \"compressed_logs\": {},\n    \"deleted_dirs\": {},\n    \"skipped_log_compress\": {}\n  }}\n}}\n",
        report_id,
        triage_id,
        json_escape(&summary_path.display().to_string()),
        json_escape(&target),
        json_escape(&input),
        json_escape(&input_sha256),
        json_escape(&verdict),
        retention.compressed_logs,
        retention.deleted_dirs,
        retention.skipped_log_compress
    );
    let meta_path = report_dir.join("meta.json");
    fs::write(&meta_path, meta_json)
        .map_err(|e| format!("failed to write '{}': {e}", meta_path.display()))?;

    let stack_lines = if signature_top3.is_empty() {
        vec!["(no signature)".to_string()]
    } else {
        signature_top3
    };
    let stack_text = stack_lines
        .iter()
        .map(|s| format!("- {}", s))
        .collect::<Vec<_>>()
        .join("\n");

    let report_md = format!(
        "# Report\n\n## Summary\n- {}: verdict `{}` on input `{}`\n\n## Reproduction Steps\n1. Image hash/version metadata: see `meta.json`\n2. PRNG seed: N/A (triage replay)\n3. Timeout: {} seconds\n4. Run: `tool triage --target {} --input '{}' --repro-retries {} --timeout-sec {}`\n\n## PoC\n- Input: `{}`\n- sha256: `{}`\n\n## Impact\n- Observed crash signature and parser/runtime failure requires manual impact confirmation.\n\n## Exploit Scenario\n- Requires crafted input delivered to `{}` parsing path.\n\n## Value\n- Automated repro evidence prepared for bug bounty triage.\n\n## Stack Top3\n{}\n",
        target,
        verdict,
        input,
        timeout_sec,
        target,
        shell_escape_single_quoted(&input),
        repro_retries,
        timeout_sec,
        input,
        input_sha256,
        target,
        stack_text
    );
    let report_md_path = report_dir.join("report.md");
    fs::write(&report_md_path, report_md)
        .map_err(|e| format!("failed to write '{}': {e}", report_md_path.display()))?;

    println!("[report] done");
    println!("source_triage: {}", triage_dir.display());
    println!("report_dir: {}", report_dir.display());
    println!("report: {}", report_md_path.display());
    println!(
        "evidence: {}, {}, {}",
        crash_report_path.display(),
        repro_path.display(),
        meta_path.display()
    );
    println!(
        "retention: compressed_logs={}, deleted_dirs={}, skipped_log_compress={}",
        retention.compressed_logs, retention.deleted_dirs, retention.skipped_log_compress
    );
    Ok(())
}

fn find_latest_triage_summary(data_dir: &Path) -> Result<(u64, PathBuf, PathBuf), String> {
    let triage_root = data_dir.join("triage");
    if !triage_root.exists() {
        return Err(format!("triage directory not found: {}", triage_root.display()));
    }

    let mut selected: Option<(u64, PathBuf, PathBuf)> = None;
    for entry in fs::read_dir(&triage_root)
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
        let Ok(id) = id_text.parse::<u64>() else {
            continue;
        };
        let summary = path.join("summary.json");
        if !summary.exists() {
            continue;
        }
        match &selected {
            Some((best, _, _)) if id <= *best => {}
            _ => selected = Some((id, path.clone(), summary)),
        }
    }

    selected.ok_or_else(|| "no triage summary found under data/triage".to_string())
}

fn extract_json_string_field(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\": \"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let mut escaped = false;
    let mut out = String::new();
    for ch in rest.chars() {
        if escaped {
            out.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

fn extract_json_u64_field(json: &str, key: &str) -> Option<u64> {
    let key_pattern = format!("\"{}\":", key);
    let start = json.find(&key_pattern)? + key_pattern.len();
    let rest = &json[start..];
    let mut digits = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if !digits.is_empty() {
            break;
        }
        if !ch.is_ascii_whitespace() {
            return None;
        }
    }
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

fn extract_first_signature_top3(summary: &str) -> Vec<String> {
    let key = "\"signature_top3\": [";
    let Some(start) = summary.find(key) else {
        return Vec::new();
    };
    let rest = &summary[start + key.len()..];
    let Some(end) = rest.find(']') else {
        return Vec::new();
    };
    let section = &rest[..end];
    let mut items = Vec::new();
    let mut in_str = false;
    let mut escaped = false;
    let mut buf = String::new();
    for ch in section.chars() {
        if !in_str {
            if ch == '"' {
                in_str = true;
                buf.clear();
            }
            continue;
        }
        if escaped {
            buf.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                in_str = false;
                items.push(buf.clone());
                if items.len() == 3 {
                    break;
                }
            }
            _ => buf.push(ch),
        }
    }
    items
}

fn build_crash_report(triage_dir: &Path, summary: &str) -> String {
    let mut lines = Vec::new();
    lines.push("== summary.json excerpt ==".to_string());
    lines.extend(excerpt_lines(summary, 50, 50));
    lines.push(String::new());
    let attempt1 = triage_dir.join("attempt-1.log");
    if let Ok(log) = fs::read_to_string(&attempt1) {
        lines.push("== attempt-1.log excerpt ==".to_string());
        lines.extend(excerpt_lines(&log, 50, 50));
    } else {
        lines.push("attempt-1.log not found".to_string());
    }
    lines.join("\n") + "\n"
}

fn excerpt_lines(text: &str, head: usize, tail: usize) -> Vec<String> {
    let lines = text.lines().map(|s| s.to_string()).collect::<Vec<_>>();
    if lines.len() <= head + tail {
        return lines;
    }
    let mut out = Vec::new();
    out.extend_from_slice(&lines[..head]);
    out.push(format!("... ({} lines omitted) ...", lines.len() - head - tail));
    out.extend_from_slice(&lines[lines.len() - tail..]);
    out
}

fn shell_escape_single_quoted(input: &str) -> String {
    input.replace('\'', "'\"'\"'")
}
