use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    json_utils::json_escape,
    target::{target_label, TargetKind},
};

pub(crate) struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0xa076_1d64_78bd_642f,
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    pub(crate) fn index(&mut self, len: usize) -> usize {
        if len <= 1 {
            0
        } else {
            (self.next_u64() as usize) % len
        }
    }
}

pub(crate) struct MutationOutput {
    pub(crate) bytes: Vec<u8>,
    pub(crate) operator_params: Vec<(&'static str, String)>,
    pub(crate) parse_preserving: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OperatorError {
    NoApplicableField,
}

pub(crate) struct MutationReport {
    pub(crate) strategy: &'static str,
    pub(crate) input_size: usize,
    pub(crate) output_size: usize,
}

pub(crate) struct BatchMutationReport {
    pub(crate) generated: usize,
    pub(crate) requested: usize,
    pub(crate) input_count: usize,
    pub(crate) out_dir: String,
    pub(crate) manifest_path: String,
}

pub(crate) struct MutationManifestEntry {
    pub(crate) index: usize,
    pub(crate) source_seed: String,
    pub(crate) source_hash: String,
    pub(crate) output_path: String,
    pub(crate) output_hash: String,
    pub(crate) operator: &'static str,
    pub(crate) operator_params: Vec<(&'static str, String)>,
    pub(crate) mutation_level: u32,
    pub(crate) parse_preserving: &'static str,
    pub(crate) validation_status: &'static str,
    pub(crate) seed: u64,
    pub(crate) input_size: usize,
    pub(crate) output_size: usize,
}

pub(crate) fn flip_value_byte(
    out: &mut [u8],
    start: usize,
    end: usize,
    rng: &mut DeterministicRng,
) -> Option<usize> {
    let end = end.min(out.len());
    if start >= end {
        return None;
    }
    let span = end - start;
    let idx = start + rng.index(span);
    let bit = rng.index(8) as u32;
    let mask = 1u8 << bit;
    out[idx] ^= mask;
    Some(idx)
}

pub(crate) fn select_operator(set: &[&'static str], rng: &mut DeterministicRng) -> &'static str {
    set[rng.index(set.len())]
}

pub(crate) fn operator_set<'a>(
    operators: &'a [&'static str],
    default_set: &'a [&'static str],
) -> &'a [&'static str] {
    if operators.is_empty() {
        default_set
    } else {
        operators
    }
}

pub(crate) fn write_output(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create output dir '{}': {e}", parent.display()))?;
        }
    }
    fs::write(out, bytes).map_err(|e| format!("failed to write output '{}': {e}", out.display()))
}

pub(crate) fn single_manifest_path(out: &Path) -> PathBuf {
    let file_name = out
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mutation");
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{}.manifest.json", file_name))
}

pub(crate) fn now_unix_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "not_available".to_string())
}

pub(crate) fn tool_commit() -> String {
    use std::process::Command;
    match Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "not_available".to_string(),
    }
}

pub(crate) fn captured_command() -> String {
    std::env::args().collect::<Vec<_>>().join(" ")
}

pub(crate) fn print_mutation_report(
    target: &TargetKind,
    input: &Path,
    out: &Path,
    seed: u64,
    report: &MutationReport,
) {
    println!("[mutate] done");
    println!("target: {}", target_label(target));
    println!("input: {}", input.display());
    println!("out: {}", out.display());
    println!("seed: {seed}");
    println!("strategy: {}", report.strategy);
    println!("input_size: {}", report.input_size);
    println!("output_size: {}", report.output_size);
}

pub(crate) fn print_batch_mutation_report(
    target: &TargetKind,
    input_dir: &Path,
    seed: u64,
    report: &BatchMutationReport,
) {
    println!("[mutate] done");
    println!("mode: batch");
    println!("target: {}", target_label(target));
    println!("input_dir: {}", input_dir.display());
    println!("out_dir: {}", report.out_dir);
    println!("seed: {seed}");
    println!("requested: {}", report.requested);
    println!("generated: {}", report.generated);
    println!("input_count: {}", report.input_count);
    println!("manifest: {}", report.manifest_path);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_mutation_manifest(
    manifest_path: &Path,
    target: &TargetKind,
    mode: &str,
    source_path: &str,
    source_lineage: &str,
    out_dir_path: &str,
    batch_id: &str,
    requested: usize,
    seed: u64,
    operators_requested: &[&'static str],
    generator: &str,
    entries: &[MutationManifestEntry],
) -> Result<PathBuf, String> {
    let generated = entries.len();
    let total_bytes: usize = entries.iter().map(|e| e.output_size).sum();
    let generated_at = now_unix_string();
    let tool_commit_str = tool_commit();
    let command = captured_command();

    let operators_json = operators_requested
        .iter()
        .map(|op| format!("\"{}\"", json_escape(op)))
        .collect::<Vec<_>>()
        .join(", ");

    let file_hashes_json = entries
        .iter()
        .map(|entry| {
            let file_name = Path::new(&entry.output_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("not_available");
            format!(
                "    \"{}\": \"{}\"",
                json_escape(file_name),
                json_escape(&entry.output_hash)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let entries_json = entries
        .iter()
        .map(|entry| {
            let params_json = entry
                .operator_params
                .iter()
                .map(|(k, v)| format!("\"{}\": \"{}\"", json_escape(k), json_escape(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    {{\"index\": {}, \"source_seed\": \"{}\", \"source_hash\": \"{}\", \"output_path\": \"{}\", \"output_hash\": \"{}\", \"operator\": \"{}\", \"operator_params\": {{{}}}, \"mutation_level\": {}, \"parse_preserving\": \"{}\", \"validation_status\": \"{}\", \"seed\": {}, \"input_size\": {}, \"output_size\": {}}}",
                entry.index,
                json_escape(&entry.source_seed),
                json_escape(&entry.source_hash),
                json_escape(&entry.output_path),
                json_escape(&entry.output_hash),
                json_escape(entry.operator),
                params_json,
                entry.mutation_level,
                json_escape(entry.parse_preserving),
                json_escape(entry.validation_status),
                entry.seed,
                entry.input_size,
                entry.output_size
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let body = format!(
        "{{\n  \"schema_version\": \"2.0\",\n  \"corpus_class\": \"mutated\",\n  \"target\": \"{}\",\n  \"mode\": \"{}\",\n  \"source_path\": \"{}\",\n  \"source_lineage\": \"{}\",\n  \"out_dir\": \"{}\",\n  \"batch_id\": \"{}\",\n  \"requested\": {},\n  \"generated\": {},\n  \"file_count\": {},\n  \"total_bytes\": {},\n  \"seed\": {},\n  \"operators_requested\": [{}],\n  \"generator\": \"{}\",\n  \"generator_version\": \"{}\",\n  \"tool_commit\": \"{}\",\n  \"command\": \"{}\",\n  \"generated_at\": {},\n  \"machine_label\": \"not_available\",\n  \"notes\": \"not_available\",\n  \"validation_status\": \"skipped\",\n  \"file_hashes\": {{\n{}\n  }},\n  \"entries\": [\n{}\n  ]\n}}\n",
        json_escape(target_label(target)),
        json_escape(mode),
        json_escape(source_path),
        json_escape(source_lineage),
        json_escape(out_dir_path),
        json_escape(batch_id),
        requested,
        generated,
        generated,
        total_bytes,
        seed,
        operators_json,
        json_escape(generator),
        env!("CARGO_PKG_VERSION"),
        json_escape(&tool_commit_str),
        json_escape(&command),
        generated_at,
        file_hashes_json,
        entries_json
    );
    fs::write(manifest_path, body).map_err(|e| {
        format!(
            "failed to write mutation manifest '{}': {e}",
            manifest_path.display()
        )
    })?;
    Ok(manifest_path.to_path_buf())
}
