use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use crate::common::{AppPaths, artifact_contract_for_data_dir, now_unix_millis, sha256_file};
use crate::json_utils::{
    extract_first_signature_top3_list, extract_json_string_literal, extract_json_u64_field,
    json_escape,
};
use crate::retention::apply_retention_policy;

pub(crate) fn run_report_pipeline(app_paths: &AppPaths) -> Result<(), String> {
    let retention = apply_retention_policy(app_paths, 30)?;
    let (triage_id, triage_dir, summary_path) = find_latest_triage_summary(&app_paths.data_dir)?;
    let summary = fs::read_to_string(&summary_path)
        .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;

    let target =
        extract_json_string_literal(&summary, "target").unwrap_or_else(|| "unknown".to_string());
    let input =
        extract_json_string_literal(&summary, "input").unwrap_or_else(|| "unknown".to_string());
    let verdict =
        extract_json_string_literal(&summary, "verdict").unwrap_or_else(|| "unknown".to_string());

    // verdict gate per specs.md §4: only "reproduced" (High Confidence) generates a report.
    // Others are queued for Manual Review (§4 line 148).
    if verdict != "reproduced" {
        record_manual_review(app_paths, triage_id, &target, &input, &verdict, &summary_path)?;
        return Err(format!(
            "report skipped: verdict '{verdict}' != 'reproduced' (triage {triage_id}); queued for manual review"
        ));
    }

    let repro_retries = extract_json_u64_field(&summary, "repro_retries").unwrap_or(3);
    let timeout_sec = extract_json_u64_field(&summary, "timeout_sec").unwrap_or(60);
    let signature_top3 = extract_first_signature_top3_list(&summary);

    let input_sha256 = if input != "unknown" {
        sha256_file(Path::new(&input)).unwrap_or_else(|_| "unavailable".to_string())
    } else {
        "unavailable".to_string()
    };

    let report_id = now_unix_millis();
    let report_dir = app_paths
        .data_dir
        .join("reports")
        .join(format!("report-{report_id}"));
    fs::create_dir_all(&report_dir)
        .map_err(|e| format!("failed to create report dir '{}': {e}", report_dir.display()))?;
    let poc = collect_report_poc(
        &report_dir,
        triage_id,
        &target,
        &input,
        &input_sha256,
        &verdict,
    );
    let repro_input = poc.repro_input_or(&input);

    let crash_report = build_crash_report(&triage_dir, &summary);
    let crash_report_path = report_dir.join("crash_report.txt");
    fs::write(&crash_report_path, crash_report)
        .map_err(|e| format!("failed to write '{}': {e}", crash_report_path.display()))?;

    let repro_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\ntool triage --target {} --input '{}' --repro-retries {} --timeout-sec {}\n",
        target,
        shell_escape_single_quoted(&repro_input),
        repro_retries,
        timeout_sec
    );
    let repro_path = report_dir.join("repro.sh");
    fs::write(&repro_path, repro_script)
        .map_err(|e| format!("failed to write '{}': {e}", repro_path.display()))?;

    let meta_json = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"report_id\": \"{}\",\n  \"source_triage_id\": \"{}\",\n  \"source_summary\": \"{}\",\n  \"target\": \"{}\",\n  \"input\": \"{}\",\n  \"input_sha256\": \"{}\",\n  \"verdict\": \"{}\",\n  \"poc_collected\": {},\n  \"poc_path\": \"{}\",\n  \"poc_sha256\": \"{}\",\n  \"poc_size_bytes\": {},\n  \"poc_source_input\": \"{}\",\n  \"poc_error\": \"{}\",\n  \"retention_days\": 30,\n  \"retention\": {{\n    \"compressed_logs\": {},\n    \"deleted_dirs\": {},\n    \"skipped_log_compress\": {}\n  }}\n}}\n",
        report_id,
        triage_id,
        json_escape(&summary_path.display().to_string()),
        json_escape(&target),
        json_escape(&input),
        json_escape(&input_sha256),
        json_escape(&verdict),
        if poc.collected { "true" } else { "false" },
        json_escape(&poc.path),
        json_escape(&poc.sha256),
        poc.size_bytes,
        json_escape(&input),
        json_escape(&poc.error),
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
        "# {}\n\n## Summary\nA reproduced crash was observed while processing a `{}` model input.\n\n- Target: `{}`\n- Verdict: `{}`\n- Source input: `{}`\n- Evidence manifest: `manifest.json`\n\n## Steps to Reproduce\n1. Review `meta.json` for source triage metadata and input hashes.\n2. Use the collected PoC input when available: `{}`.\n3. Run: `tool triage --target {} --input '{}' --repro-retries {} --timeout-sec {}`\n4. Compare the observed crash signature with `crash_report.txt` and the stack frames below.\n\n## Impact\nObserved crash signature and parser/runtime failure require manual impact confirmation. Treat this as a submission candidate, not an automatic exploitability conclusion.\n\n## Suggested Fix\nValidate parser assumptions around the crashing input path, add regression coverage for the PoC, and reject malformed model files before reaching the crashing code path.\n\n## PoC\n- Original input: `{}`\n- Original sha256: `{}`\n- Collected copy: `{}`\n- Collection status: `{}`\n\n## Exploit Scenario\nA crafted model file reaches the `{}` parsing path and triggers the reproduced crash condition.\n\n## Stack Top3\n{}\n",
        build_report_title(&target, &stack_lines),
        target,
        target,
        verdict,
        input,
        if poc.path.is_empty() { "-" } else { &poc.path },
        target,
        shell_escape_single_quoted(&repro_input),
        repro_retries,
        timeout_sec,
        input,
        input_sha256,
        if poc.path.is_empty() { "-" } else { &poc.path },
        if poc.collected { "collected" } else { "not_collected" },
        target,
        stack_text
    );
    let report_md_path = report_dir.join("report.md");
    fs::write(&report_md_path, report_md)
        .map_err(|e| format!("failed to write '{}': {e}", report_md_path.display()))?;

    let manifest_path = write_report_manifest(&report_dir, report_id, triage_id, &target, &verdict, &poc)?;
    let bundle_path = write_evidence_zip(&report_dir, report_id, &poc)?;

    println!("[report] done");
    println!("source_triage: {}", triage_dir.display());
    println!("report_dir: {}", report_dir.display());
    println!("report: {}", report_md_path.display());
    println!(
        "evidence: {}, {}, {}, {}",
        crash_report_path.display(),
        repro_path.display(),
        meta_path.display(),
        manifest_path.display()
    );
    println!("bundle: {}", bundle_path.display());
    if poc.collected {
        println!("poc: {}", poc.path);
    } else if !poc.error.is_empty() {
        println!("poc: {}", poc.error);
    }
    println!(
        "retention: compressed_logs={}, deleted_dirs={}, skipped_log_compress={}",
        retention.compressed_logs, retention.deleted_dirs, retention.skipped_log_compress
    );
    Ok(())
}

fn find_latest_triage_summary(data_dir: &Path) -> Result<(u128, PathBuf, PathBuf), String> {
    let triage_root = artifact_contract_for_data_dir(data_dir).triage_root;
    if !triage_root.exists() {
        return Err(format!("triage directory not found: {}", triage_root.display()));
    }

    let mut selected: Option<(u128, PathBuf, PathBuf)> = None;
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
        let Ok(id) = id_text.parse::<u128>() else {
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

fn build_report_title(target: &str, stack_lines: &[String]) -> String {
    let signature = stack_lines
        .iter()
        .find(|s| s.as_str() != "(no signature)")
        .map(|s| s.as_str())
        .unwrap_or("reproduced crash");
    format!("{target} reproduced crash candidate: {signature}")
}

fn write_report_manifest(
    report_dir: &Path,
    report_id: u128,
    triage_id: u128,
    target: &str,
    verdict: &str,
    poc: &PocCollection,
) -> Result<PathBuf, String> {
    let mut relative_paths = vec![
        "report.md".to_string(),
        "repro.sh".to_string(),
        "meta.json".to_string(),
        "crash_report.txt".to_string(),
    ];
    if let Some(path) = relative_report_path(report_dir, &poc.path) {
        relative_paths.push(path);
    }

    let mut file_entries = Vec::new();
    for relative_path in relative_paths {
        let path = report_dir.join(&relative_path);
        let metadata = fs::metadata(&path)
            .map_err(|e| format!("failed to stat report artifact '{}': {e}", path.display()))?;
        let sha256 = sha256_file(&path)
            .map_err(|e| format!("failed to hash report artifact '{}': {e}", path.display()))?;
        file_entries.push(format!(
            "    {{\"path\":\"{}\",\"sha256\":\"{}\",\"size_bytes\":{}}}",
            json_escape(&relative_path),
            json_escape(&sha256),
            metadata.len()
        ));
    }

    let manifest = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"bundle_type\": \"report_evidence\",\n  \"report_id\": \"{}\",\n  \"source_triage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"verdict\": \"{}\",\n  \"generated_at\": {},\n  \"hash_algorithm\": \"sha256\",\n  \"manifest_path\": \"manifest.json\",\n  \"manifest_self_hash\": \"not_self_hashed\",\n  \"files\": [\n{}\n  ]\n}}\n",
        report_id,
        triage_id,
        json_escape(target),
        json_escape(verdict),
        now_unix_millis(),
        file_entries.join(",\n")
    );
    let manifest_path = report_dir.join("manifest.json");
    fs::write(&manifest_path, manifest)
        .map_err(|e| format!("failed to write '{}': {e}", manifest_path.display()))?;
    Ok(manifest_path)
}

fn write_evidence_zip(report_dir: &Path, report_id: u128, poc: &PocCollection) -> Result<PathBuf, String> {
    let mut relative_paths = vec![
        "report.md".to_string(),
        "repro.sh".to_string(),
        "meta.json".to_string(),
        "crash_report.txt".to_string(),
        "manifest.json".to_string(),
    ];
    if let Some(path) = relative_report_path(report_dir, &poc.path) {
        relative_paths.push(path);
    }

    let bundle_path = report_dir.join(format!("report-{report_id}-evidence.zip"));
    write_store_zip(&bundle_path, report_dir, &relative_paths)?;
    Ok(bundle_path)
}

fn write_store_zip(zip_path: &Path, root: &Path, relative_paths: &[String]) -> Result<(), String> {
    let mut out = File::create(zip_path)
        .map_err(|e| format!("failed to create evidence zip '{}': {e}", zip_path.display()))?;
    let mut central_entries = Vec::new();
    let mut offset = 0u64;

    for relative_path in relative_paths {
        let input_path = root.join(relative_path);
        let data = fs::read(&input_path)
            .map_err(|e| format!("failed to read zip input '{}': {e}", input_path.display()))?;
        let name = relative_path.replace('\\', "/");
        let name_bytes = name.as_bytes();
        let size = checked_zip_u32(data.len() as u64, "file size")?;
        let name_len = checked_zip_u16(name_bytes.len(), "file name length")?;
        let crc = crc32(&data);
        let local_offset = checked_zip_u32(offset, "local header offset")?;

        write_u32_le(&mut out, 0x0403_4b50)?;
        write_u16_le(&mut out, 20)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 33)?;
        write_u32_le(&mut out, crc)?;
        write_u32_le(&mut out, size)?;
        write_u32_le(&mut out, size)?;
        write_u16_le(&mut out, name_len)?;
        write_u16_le(&mut out, 0)?;
        out.write_all(name_bytes)
            .map_err(|e| format!("failed to write zip file name: {e}"))?;
        out.write_all(&data)
            .map_err(|e| format!("failed to write zip file data: {e}"))?;

        offset += 30 + name_bytes.len() as u64 + data.len() as u64;
        central_entries.push(ZipCentralEntry {
            name,
            crc,
            size,
            local_offset,
        });
    }

    let central_dir_offset = offset;
    for entry in &central_entries {
        let name_bytes = entry.name.as_bytes();
        let name_len = checked_zip_u16(name_bytes.len(), "central file name length")?;

        write_u32_le(&mut out, 0x0201_4b50)?;
        write_u16_le(&mut out, 20)?;
        write_u16_le(&mut out, 20)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 33)?;
        write_u32_le(&mut out, entry.crc)?;
        write_u32_le(&mut out, entry.size)?;
        write_u32_le(&mut out, entry.size)?;
        write_u16_le(&mut out, name_len)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u16_le(&mut out, 0)?;
        write_u32_le(&mut out, 0)?;
        write_u32_le(&mut out, entry.local_offset)?;
        out.write_all(name_bytes)
            .map_err(|e| format!("failed to write central zip file name: {e}"))?;

        offset += 46 + name_bytes.len() as u64;
    }

    let central_dir_size = checked_zip_u32(offset - central_dir_offset, "central directory size")?;
    let central_dir_offset = checked_zip_u32(central_dir_offset, "central directory offset")?;
    let entry_count = checked_zip_u16(central_entries.len(), "zip entry count")?;

    write_u32_le(&mut out, 0x0605_4b50)?;
    write_u16_le(&mut out, 0)?;
    write_u16_le(&mut out, 0)?;
    write_u16_le(&mut out, entry_count)?;
    write_u16_le(&mut out, entry_count)?;
    write_u32_le(&mut out, central_dir_size)?;
    write_u32_le(&mut out, central_dir_offset)?;
    write_u16_le(&mut out, 0)?;
    Ok(())
}

struct ZipCentralEntry {
    name: String,
    crc: u32,
    size: u32,
    local_offset: u32,
}

fn write_u16_le(out: &mut File, value: u16) -> Result<(), String> {
    out.write_all(&value.to_le_bytes())
        .map_err(|e| format!("failed to write zip u16: {e}"))
}

fn write_u32_le(out: &mut File, value: u32) -> Result<(), String> {
    out.write_all(&value.to_le_bytes())
        .map_err(|e| format!("failed to write zip u32: {e}"))
}

fn checked_zip_u16(value: usize, label: &str) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| format!("{label} exceeds ZIP32 limit: {value}"))
}

fn checked_zip_u32(value: u64, label: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{label} exceeds ZIP32 limit: {value}"))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn relative_report_path(report_dir: &Path, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let path = Path::new(path);
    let relative = path.strip_prefix(report_dir).ok()?;
    Some(relative.display().to_string().replace('\\', "/"))
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

struct PocCollection {
    collected: bool,
    path: String,
    sha256: String,
    size_bytes: u64,
    error: String,
}

impl PocCollection {
    fn repro_input_or(&self, default_input: &str) -> String {
        if self.collected && !self.path.is_empty() {
            self.path.clone()
        } else {
            default_input.to_string()
        }
    }
}

fn collect_report_poc(
    report_dir: &Path,
    triage_id: u128,
    target: &str,
    input: &str,
    input_sha256: &str,
    verdict: &str,
) -> PocCollection {
    if verdict != "reproduced" {
        return PocCollection {
            collected: false,
            path: String::new(),
            sha256: String::new(),
            size_bytes: 0,
            error: "skipped_non_reproduced".to_string(),
        };
    }

    let input_path = Path::new(input);
    if !input_path.exists() || !input_path.is_file() {
        return PocCollection {
            collected: false,
            path: String::new(),
            sha256: String::new(),
            size_bytes: 0,
            error: "source_input_not_found".to_string(),
        };
    }

    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let hash12 = short_hash12(input_sha256);
    let target_safe = sanitize_filename_component(target);
    let filename = format!("poc-{target_safe}-triage-{triage_id}-{hash12}{ext}");
    let poc_dir = report_dir.join("poc");
    if let Err(e) = fs::create_dir_all(&poc_dir) {
        return PocCollection {
            collected: false,
            path: String::new(),
            sha256: String::new(),
            size_bytes: 0,
            error: format!("poc_dir_create_failed:{e}"),
        };
    }

    let dst = poc_dir.join(filename);
    if let Err(e) = fs::copy(input_path, &dst) {
        return PocCollection {
            collected: false,
            path: String::new(),
            sha256: String::new(),
            size_bytes: 0,
            error: format!("poc_copy_failed:{e}"),
        };
    }

    let size_bytes = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
    let poc_sha256 = sha256_file(&dst).unwrap_or_else(|_| "unavailable".to_string());
    PocCollection {
        collected: true,
        path: dst.display().to_string(),
        sha256: poc_sha256,
        size_bytes,
        error: String::new(),
    }
}

fn short_hash12(input_sha256: &str) -> String {
    let hex = input_sha256
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>();
    if hex.len() >= 12 {
        hex[..12].to_string()
    } else if !hex.is_empty() {
        hex
    } else {
        "unknownhash".to_string()
    }
}

fn sanitize_filename_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "target".to_string()
    } else {
        trimmed.to_string()
    }
}

fn record_manual_review(
    app_paths: &AppPaths,
    triage_id: u128,
    target: &str,
    input: &str,
    verdict: &str,
    summary_path: &Path,
) -> Result<(), String> {
    let review_dir = app_paths.data_dir.join("manual_review");
    fs::create_dir_all(&review_dir).map_err(|e| {
        format!("failed to create manual_review dir '{}': {e}", review_dir.display())
    })?;
    let review_path = review_dir.join(format!("triage-{triage_id}.json"));
    let body = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"triage_id\": \"{}\",\n  \"target\": \"{}\",\n  \"input\": \"{}\",\n  \"verdict\": \"{}\",\n  \"summary_path\": \"{}\",\n  \"queued_at\": {}\n}}\n",
        triage_id,
        json_escape(target),
        json_escape(input),
        json_escape(verdict),
        json_escape(&summary_path.display().to_string()),
        now_unix_millis()
    );
    fs::write(&review_path, body).map_err(|e| {
        format!("failed to write manual review '{}': {e}", review_path.display())
    })?;
    println!("[manual_review] queued: {}", review_path.display());
    Ok(())
}
