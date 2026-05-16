use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    common::sha256_file,
    json_utils::json_escape,
    target::{target_label, TargetKind},
};

pub(crate) mod attribute;
pub(crate) mod dtype;
pub(crate) mod graph_metadata;
pub(crate) mod initializer_metadata;
pub(crate) mod name;
pub(crate) mod shape;

struct MutationReport {
    strategy: &'static str,
    input_size: usize,
    output_size: usize,
}

struct BatchMutationReport {
    generated: usize,
    requested: usize,
    input_count: usize,
    out_dir: String,
    manifest_path: String,
}

struct MutationManifestEntry {
    index: usize,
    source_seed: String,
    source_hash: String,
    output_path: String,
    output_hash: String,
    operator: &'static str,
    operator_params: Vec<(&'static str, String)>,
    mutation_level: u32,
    parse_preserving: &'static str,
    validation_status: &'static str,
    seed: u64,
    input_size: usize,
    output_size: usize,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct ProtoField {
    wire_type: u8,
    value_start: usize,
    value_end: usize,
}

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldRef {
    pub(crate) field_number: u32,
    pub(crate) wire_type: u8,
    pub(crate) value_start: usize,
    pub(crate) value_end: usize,
}

pub(crate) fn find_fields(bytes: &[u8], path: &[(u32, u8)]) -> Vec<FieldRef> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut ranges: Vec<(usize, usize)> = vec![(0, bytes.len())];
    for (i, &(fnum, wtype)) in path.iter().enumerate() {
        let is_leaf = i + 1 == path.len();
        if is_leaf {
            let mut leaf = Vec::new();
            for &(start, end) in &ranges {
                for f in scan_proto_range(bytes, start, end) {
                    if f.field_number == fnum && f.wire_type == wtype {
                        leaf.push(f);
                    }
                }
            }
            return leaf;
        }
        if wtype != 2 {
            return Vec::new();
        }
        let mut next_ranges = Vec::new();
        for &(start, end) in &ranges {
            for f in scan_proto_range(bytes, start, end) {
                if f.field_number == fnum && f.wire_type == 2 {
                    next_ranges.push((f.value_start, f.value_end));
                }
            }
        }
        if next_ranges.is_empty() {
            return Vec::new();
        }
        ranges = next_ranges;
    }
    Vec::new()
}

pub(crate) fn pick_field<'a>(
    fields: &'a [FieldRef],
    rng: &mut DeterministicRng,
) -> Option<&'a FieldRef> {
    if fields.is_empty() {
        None
    } else {
        let idx = rng.index(fields.len());
        Some(&fields[idx])
    }
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

pub(crate) fn flip_varint_byte(
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
    let bit = rng.index(7) as u32;
    let mask = 1u8 << bit;
    out[idx] ^= mask;
    Some(idx)
}

fn scan_proto_range(bytes: &[u8], start: usize, end: usize) -> Vec<FieldRef> {
    let mut fields = Vec::new();
    let mut cursor = start;
    let end = end.min(bytes.len());
    while cursor < end && fields.len() < 4096 {
        let Some((tag, after_tag)) = read_varint_in(bytes, cursor, end) else {
            break;
        };
        if tag == 0 {
            break;
        }
        let wire_type = (tag & 0x7) as u8;
        let field_number = (tag >> 3) as u32;
        if field_number == 0 {
            break;
        }
        let (value_start, value_end) = match wire_type {
            0 => {
                let Some((_, ve)) = read_varint_in(bytes, after_tag, end) else {
                    break;
                };
                (after_tag, ve)
            }
            1 => {
                let Some(ve) = after_tag.checked_add(8) else {
                    break;
                };
                if ve > end {
                    break;
                }
                (after_tag, ve)
            }
            2 => {
                let Some((len, vs)) = read_varint_in(bytes, after_tag, end) else {
                    break;
                };
                let Ok(len) = usize::try_from(len) else {
                    break;
                };
                let Some(ve) = vs.checked_add(len) else {
                    break;
                };
                if ve > end {
                    break;
                }
                (vs, ve)
            }
            5 => {
                let Some(ve) = after_tag.checked_add(4) else {
                    break;
                };
                if ve > end {
                    break;
                }
                (after_tag, ve)
            }
            _ => break,
        };
        fields.push(FieldRef {
            field_number,
            wire_type,
            value_start,
            value_end,
        });
        cursor = value_end;
    }
    fields
}

fn read_varint_in(bytes: &[u8], start: usize, end: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut cursor = start;
    let end = end.min(bytes.len());
    while cursor < end && shift < 64 {
        let byte = bytes[cursor];
        value |= ((byte & 0x7f) as u64) << shift;
        cursor += 1;
        if byte & 0x80 == 0 {
            return Some((value, cursor));
        }
        shift += 7;
    }
    None
}

pub(crate) const DEFAULT_OPERATORS: &[&str] = &[
    shape::NAME,
    dtype::NAME,
    name::NAME,
    attribute::NAME,
    initializer_metadata::NAME,
    graph_metadata::NAME,
];

pub(crate) const KNOWN_OPERATORS: &[&str] = DEFAULT_OPERATORS;

pub(crate) fn validate_operators(requested: &[String]) -> Result<Vec<&'static str>, String> {
    let mut resolved = Vec::with_capacity(requested.len());
    for name in requested {
        let matched = KNOWN_OPERATORS
            .iter()
            .copied()
            .find(|known| *known == name.as_str())
            .ok_or_else(|| {
                format!(
                    "unknown operator '{}'; known operators: {}",
                    name,
                    KNOWN_OPERATORS.join(", ")
                )
            })?;
        resolved.push(matched);
    }
    Ok(resolved)
}

fn select_operator(set: &[&'static str], rng: &mut DeterministicRng) -> &'static str {
    set[rng.index(set.len())]
}

fn dispatch(
    operator: &'static str,
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    match operator {
        "shape" => shape::apply(bytes, rng),
        "dtype" => dtype::apply(bytes, rng),
        "name" => name::apply(bytes, rng),
        "attribute" => attribute::apply(bytes, rng),
        "initializer_metadata" => initializer_metadata::apply(bytes, rng),
        "graph_metadata" => graph_metadata::apply(bytes, rng),
        _ => Err(OperatorError::NoApplicableField),
    }
}

fn operator_set<'a>(operators: &'a [&'static str]) -> &'a [&'static str] {
    if operators.is_empty() {
        DEFAULT_OPERATORS
    } else {
        operators
    }
}

pub(crate) fn run(
    target: &TargetKind,
    input: Option<&Path>,
    out: Option<&Path>,
    input_dir: Option<&Path>,
    out_dir: Option<&Path>,
    count: usize,
    seed: u64,
    operators: &[&'static str],
) -> Result<(), String> {
    match (input, out, input_dir, out_dir) {
        (Some(input), Some(out), None, None) => {
            run_single_mutation(target, input, out, seed, operators)
        }
        (None, None, Some(input_dir), Some(out_dir)) => {
            run_batch_mutation(target, input_dir, out_dir, count, seed, operators)
        }
        _ => Err(
            "choose exactly one mode: --input <file> --out <file> or --input-dir <dir> --out-dir <dir>"
                .to_string(),
        ),
    }
}

fn run_single_mutation(
    target: &TargetKind,
    input: &Path,
    out: &Path,
    seed: u64,
    operators: &[&'static str],
) -> Result<(), String> {
    if !input.exists() || !input.is_file() {
        return Err(format!("input is invalid: {}", input.display()));
    }

    let bytes =
        fs::read(input).map_err(|e| format!("failed to read input '{}': {e}", input.display()))?;
    if bytes.is_empty() {
        return Err("input is empty".to_string());
    }

    let mut rng = DeterministicRng::new(seed);
    let set = operator_set(operators);
    let chosen = select_operator(set, &mut rng);
    let result = dispatch(chosen, &bytes, &mut rng)
        .map_err(|e| format!("operator '{}' failed: {:?}", chosen, e))?;
    write_output(out, &result.bytes)?;

    let output_size = result.bytes.len();
    let source_hash = sha256_file(input).unwrap_or_else(|_| "not_available".to_string());
    let output_hash = sha256_file(out).unwrap_or_else(|_| "not_available".to_string());
    let entry = MutationManifestEntry {
        index: 1,
        source_seed: input.display().to_string(),
        source_hash,
        output_path: out.display().to_string(),
        output_hash,
        operator: chosen,
        operator_params: result.operator_params,
        mutation_level: 1,
        parse_preserving: result.parse_preserving,
        validation_status: "skipped",
        seed,
        input_size: bytes.len(),
        output_size,
    };

    let manifest_path_buf = single_manifest_path(out);
    let batch_id = out
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("single")
        .to_string();
    let out_dir_str = out
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    write_mutation_manifest(
        &manifest_path_buf,
        target,
        "single",
        &input.display().to_string(),
        &format!("seed:{}", input.display()),
        &out_dir_str,
        &batch_id,
        1,
        seed,
        operators,
        &[entry],
    )?;

    let report = MutationReport {
        strategy: chosen,
        input_size: bytes.len(),
        output_size,
    };
    print_mutation_report(target, input, out, seed, &report);
    println!("manifest: {}", manifest_path_buf.display());
    Ok(())
}

fn single_manifest_path(out: &Path) -> PathBuf {
    let file_name = out
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mutation");
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{}.manifest.json", file_name))
}

fn run_batch_mutation(
    target: &TargetKind,
    input_dir: &Path,
    out_dir: &Path,
    count: usize,
    seed: u64,
    operators: &[&'static str],
) -> Result<(), String> {
    if count == 0 {
        return Err("count must be >= 1".to_string());
    }
    if !input_dir.exists() || !input_dir.is_dir() {
        return Err(format!("input_dir is invalid: {}", input_dir.display()));
    }

    let inputs = collect_onnx_inputs(input_dir)?;
    if inputs.is_empty() {
        return Err(format!(
            "no .onnx input files found in {}",
            input_dir.display()
        ));
    }
    fs::create_dir_all(out_dir)
        .map_err(|e| format!("failed to create out_dir '{}': {e}", out_dir.display()))?;

    let set = operator_set(operators);
    let mut entries = Vec::new();
    for idx in 0..count {
        let input = &inputs[idx % inputs.len()];
        let bytes = fs::read(input)
            .map_err(|e| format!("failed to read input '{}': {e}", input.display()))?;
        if bytes.is_empty() {
            continue;
        }

        let mut rng = DeterministicRng::new(seed.wrapping_add(idx as u64));
        let entry_seed = seed.wrapping_add(idx as u64);
        let chosen = select_operator(set, &mut rng);
        let result = match dispatch(chosen, &bytes, &mut rng) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let output_size = result.bytes.len();
        let out = out_dir.join(format!("mut-onnx-{:06}.onnx", idx + 1));
        fs::write(&out, &result.bytes)
            .map_err(|e| format!("failed to write output '{}': {e}", out.display()))?;
        let output_hash = sha256_file(&out).unwrap_or_else(|_| "not_available".to_string());
        let source_hash = sha256_file(input).unwrap_or_else(|_| "not_available".to_string());
        entries.push(MutationManifestEntry {
            index: idx + 1,
            source_seed: input.display().to_string(),
            source_hash,
            output_path: out.display().to_string(),
            output_hash,
            operator: chosen,
            operator_params: result.operator_params,
            mutation_level: 1,
            parse_preserving: result.parse_preserving,
            validation_status: "skipped",
            seed: entry_seed,
            input_size: bytes.len(),
            output_size,
        });
    }

    let batch_id = out_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("not_available")
        .to_string();
    let manifest_path_buf = out_dir.join("manifest.json");
    write_mutation_manifest(
        &manifest_path_buf,
        target,
        "batch",
        &input_dir.display().to_string(),
        &format!("seed_corpus:{}", input_dir.display()),
        &out_dir.display().to_string(),
        &batch_id,
        count,
        seed,
        operators,
        &entries,
    )?;

    let report = BatchMutationReport {
        generated: entries.len(),
        requested: count,
        input_count: inputs.len(),
        out_dir: out_dir.display().to_string(),
        manifest_path: manifest_path_buf.display().to_string(),
    };
    print_batch_mutation_report(target, input_dir, seed, &report);
    Ok(())
}

fn collect_onnx_inputs(input_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut inputs = Vec::new();
    for entry in fs::read_dir(input_dir)
        .map_err(|e| format!("failed to read input_dir '{}': {e}", input_dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read input_dir entry: {e}"))?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("onnx"))
                .unwrap_or(false)
        {
            inputs.push(path);
        }
    }
    inputs.sort();
    Ok(inputs)
}

#[allow(dead_code)]
fn mutate_onnx_wire(bytes: &[u8], rng: &mut DeterministicRng) -> (Vec<u8>, &'static str) {
    let mut out = bytes.to_vec();
    let fields = scan_proto_fields(bytes);

    let length_fields = fields
        .iter()
        .copied()
        .filter(|f| f.wire_type == 2 && f.value_end > f.value_start)
        .collect::<Vec<_>>();
    if !length_fields.is_empty() {
        let field = length_fields[rng.index(length_fields.len())];
        mutate_payload_byte(&mut out, field.value_start, field.value_end, rng);
        return (out, "onnx_length_delimited_payload_flip");
    }

    let varint_fields = fields
        .iter()
        .copied()
        .filter(|f| f.wire_type == 0 && f.value_end > f.value_start)
        .collect::<Vec<_>>();
    if !varint_fields.is_empty() {
        let field = varint_fields[rng.index(varint_fields.len())];
        mutate_payload_byte(&mut out, field.value_start, field.value_end, rng);
        return (out, "onnx_varint_payload_flip");
    }

    mutate_payload_byte(&mut out, 0, bytes.len(), rng);
    (out, "byte_flip_fallback")
}

#[allow(dead_code)]
fn scan_proto_fields(bytes: &[u8]) -> Vec<ProtoField> {
    let mut fields = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() && fields.len() < 256 {
        let Some((tag, after_tag)) = read_varint(bytes, cursor) else {
            break;
        };
        if tag == 0 {
            break;
        }

        let wire_type = (tag & 0x7) as u8;
        let field_number = tag >> 3;
        if field_number == 0 {
            break;
        }

        match wire_type {
            0 => {
                let value_start = after_tag;
                let Some((_, value_end)) = read_varint(bytes, value_start) else {
                    break;
                };
                fields.push(ProtoField {
                    wire_type,
                    value_start,
                    value_end,
                });
                cursor = value_end;
            }
            1 => {
                let value_start = after_tag;
                let Some(value_end) = value_start.checked_add(8) else {
                    break;
                };
                if value_end > bytes.len() {
                    break;
                }
                fields.push(ProtoField {
                    wire_type,
                    value_start,
                    value_end,
                });
                cursor = value_end;
            }
            2 => {
                let Some((len, value_start)) = read_varint(bytes, after_tag) else {
                    break;
                };
                let Ok(len) = usize::try_from(len) else {
                    break;
                };
                let Some(value_end) = value_start.checked_add(len) else {
                    break;
                };
                if value_end > bytes.len() {
                    break;
                }
                fields.push(ProtoField {
                    wire_type,
                    value_start,
                    value_end,
                });
                cursor = value_end;
            }
            5 => {
                let value_start = after_tag;
                let Some(value_end) = value_start.checked_add(4) else {
                    break;
                };
                if value_end > bytes.len() {
                    break;
                }
                fields.push(ProtoField {
                    wire_type,
                    value_start,
                    value_end,
                });
                cursor = value_end;
            }
            _ => break,
        }
    }
    fields
}

#[allow(dead_code)]
fn read_varint(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut cursor = start;
    while cursor < bytes.len() && shift < 64 {
        let byte = bytes[cursor];
        value |= ((byte & 0x7f) as u64) << shift;
        cursor += 1;
        if byte & 0x80 == 0 {
            return Some((value, cursor));
        }
        shift += 7;
    }
    None
}

#[allow(dead_code)]
fn mutate_payload_byte(out: &mut [u8], start: usize, end: usize, rng: &mut DeterministicRng) {
    if out.is_empty() {
        return;
    }
    let end = end.min(out.len());
    let start = start.min(end.saturating_sub(1));
    let span = end.saturating_sub(start).max(1);
    let idx = start + rng.index(span);
    let mask = 1u8 << (rng.index(8) as u32);
    out[idx] ^= mask;
    if out[idx] == 0 && mask != 0 {
        out[idx] = mask;
    }
}

fn write_output(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create output dir '{}': {e}", parent.display()))?;
        }
    }
    fs::write(out, bytes).map_err(|e| format!("failed to write output '{}': {e}", out.display()))
}

fn write_mutation_manifest(
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
        "{{\n  \"schema_version\": \"2.0\",\n  \"corpus_class\": \"mutated\",\n  \"target\": \"{}\",\n  \"mode\": \"{}\",\n  \"source_path\": \"{}\",\n  \"source_lineage\": \"{}\",\n  \"out_dir\": \"{}\",\n  \"batch_id\": \"{}\",\n  \"requested\": {},\n  \"generated\": {},\n  \"file_count\": {},\n  \"total_bytes\": {},\n  \"seed\": {},\n  \"operators_requested\": [{}],\n  \"generator\": \"tool/mutate/onnx\",\n  \"generator_version\": \"{}\",\n  \"tool_commit\": \"{}\",\n  \"command\": \"{}\",\n  \"generated_at\": {},\n  \"machine_label\": \"not_available\",\n  \"notes\": \"not_available\",\n  \"validation_status\": \"skipped\",\n  \"file_hashes\": {{\n{}\n  }},\n  \"entries\": [\n{}\n  ]\n}}\n",
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

fn now_unix_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "not_available".to_string())
}

fn tool_commit() -> String {
    use std::process::Command;
    match Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "not_available".to_string(),
    }
}

fn captured_command() -> String {
    std::env::args().collect::<Vec<_>>().join(" ")
}

fn print_mutation_report(
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

fn print_batch_mutation_report(
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

#[cfg(test)]
pub(crate) mod test_fixtures {
    pub(crate) fn encode_tag(field_number: u32, wire_type: u8) -> Vec<u8> {
        encode_varint(((field_number as u64) << 3) | (wire_type as u64))
    }

    pub(crate) fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
                out.push(byte);
            } else {
                out.push(byte);
                return out;
            }
        }
    }

    pub(crate) fn encode_length_delimited(field_number: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = encode_tag(field_number, 2);
        out.extend(encode_varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    pub(crate) fn encode_varint_field(field_number: u32, value: u64) -> Vec<u8> {
        let mut out = encode_tag(field_number, 0);
        out.extend(encode_varint(value));
        out
    }

    pub(crate) fn encode_string_field(field_number: u32, value: &str) -> Vec<u8> {
        encode_length_delimited(field_number, value.as_bytes())
    }
}
