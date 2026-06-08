use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    common::sha256_file,
    mutate::common::{
        print_batch_mutation_report, print_mutation_report, select_operator, single_manifest_path,
        write_mutation_manifest, write_output, BatchMutationReport, MutationManifestEntry,
        MutationReport,
    },
    target::TargetKind,
};

pub(crate) use crate::mutate::common::{
    flip_value_byte, operator_set, DeterministicRng, MutationOutput, OperatorError,
};

pub(crate) mod attribute;
pub(crate) mod byte_flip;
pub(crate) mod dtype;
pub(crate) mod dtype_valid;
pub(crate) mod dtype_wide;
pub(crate) mod graph_metadata;
pub(crate) mod havoc;
pub(crate) mod initializer_metadata;
pub(crate) mod name;
pub(crate) mod name_consistent;
pub(crate) mod shape;
pub(crate) mod shape_valid;

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

pub(crate) fn read_varint_value(bytes: &[u8], start: usize, end: usize) -> Option<u64> {
    read_varint_in(bytes, start, end).map(|(value, _)| value)
}

pub(crate) fn write_varint_same_width(
    out: &mut [u8],
    start: usize,
    end: usize,
    value: u64,
) -> Option<()> {
    let end = end.min(out.len());
    if start >= end {
        return None;
    }
    let width = end - start;
    if width > 10 {
        return None;
    }
    let mut remaining = value;
    for idx in 0..width {
        let mut byte = (remaining & 0x7f) as u8;
        remaining >>= 7;
        if idx + 1 < width {
            byte |= 0x80;
        }
        out[start + idx] = byte;
    }
    if remaining == 0 {
        Some(())
    } else {
        None
    }
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

pub(crate) const KNOWN_OPERATORS: &[&str] = &[
    shape::NAME,
    dtype::NAME,
    name::NAME,
    attribute::NAME,
    initializer_metadata::NAME,
    graph_metadata::NAME,
    shape_valid::NAME,
    dtype_valid::NAME,
    dtype_wide::NAME,
    name_consistent::NAME,
    byte_flip::NAME,
    havoc::NAME,
];

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
        "shape_valid" => shape_valid::apply(bytes, rng),
        "dtype_valid" => dtype_valid::apply(bytes, rng),
        "dtype_wide" => dtype_wide::apply(bytes, rng),
        "name_consistent" => name_consistent::apply(bytes, rng),
        "byte_flip" => byte_flip::apply(bytes, rng),
        "havoc" => havoc::apply(bytes, rng),
        _ => Err(OperatorError::NoApplicableField),
    }
}

fn mutation_level_for_operator(operator: &str) -> u32 {
    match operator {
        "shape_valid" | "dtype_valid" | "dtype_wide" | "name_consistent" => 2,
        _ => 1,
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
    let set = operator_set(operators, DEFAULT_OPERATORS);
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
        mutation_level: mutation_level_for_operator(chosen),
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
        "tool/mutate/onnx",
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

    let set = operator_set(operators, DEFAULT_OPERATORS);
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
            mutation_level: mutation_level_for_operator(chosen),
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
        "tool/mutate/onnx",
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
