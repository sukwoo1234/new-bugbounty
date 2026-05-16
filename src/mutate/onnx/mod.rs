use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    common::sha256_file,
    json_utils::json_escape,
    target::{target_label, TargetKind},
};

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
    output_path: String,
    strategy: &'static str,
    mutation_seed: u64,
    input_size: usize,
    output_size: usize,
    output_sha256: String,
}

#[derive(Clone, Copy)]
struct ProtoField {
    wire_type: u8,
    value_start: usize,
    value_end: usize,
}

struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0xa076_1d64_78bd_642f,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn index(&mut self, len: usize) -> usize {
        if len <= 1 {
            0
        } else {
            (self.next_u64() as usize) % len
        }
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
) -> Result<(), String> {
    match (input, out, input_dir, out_dir) {
        (Some(input), Some(out), None, None) => run_single_mutation(target, input, out, seed),
        (None, None, Some(input_dir), Some(out_dir)) => {
            run_batch_mutation(target, input_dir, out_dir, count, seed)
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
    let (mutated, strategy) = mutate_onnx_wire(&bytes, &mut rng);
    write_output(out, &mutated)?;

    let report = MutationReport {
        strategy,
        input_size: bytes.len(),
        output_size: mutated.len(),
    };
    print_mutation_report(target, input, out, seed, &report);
    Ok(())
}

fn run_batch_mutation(
    target: &TargetKind,
    input_dir: &Path,
    out_dir: &Path,
    count: usize,
    seed: u64,
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

    let mut entries = Vec::new();
    for idx in 0..count {
        let input = &inputs[idx % inputs.len()];
        let bytes = fs::read(input)
            .map_err(|e| format!("failed to read input '{}': {e}", input.display()))?;
        if bytes.is_empty() {
            continue;
        }

        let mut rng = DeterministicRng::new(seed.wrapping_add(idx as u64));
        let mutation_seed = seed.wrapping_add(idx as u64);
        let (mutated, strategy) = mutate_onnx_wire(&bytes, &mut rng);
        let output_size = mutated.len();
        let out = out_dir.join(format!("mut-onnx-{:06}.onnx", idx + 1));
        fs::write(&out, mutated)
            .map_err(|e| format!("failed to write output '{}': {e}", out.display()))?;
        let output_sha256 = sha256_file(&out).unwrap_or_else(|_| "unavailable".to_string());
        entries.push(MutationManifestEntry {
            index: idx + 1,
            source_seed: input.display().to_string(),
            output_path: out.display().to_string(),
            strategy,
            mutation_seed,
            input_size: bytes.len(),
            output_size,
            output_sha256,
        });
    }

    let manifest_path = write_mutation_manifest(out_dir, target, input_dir, count, seed, &entries)?;
    let report = BatchMutationReport {
        generated: entries.len(),
        requested: count,
        input_count: inputs.len(),
        out_dir: out_dir.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
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
    out_dir: &Path,
    target: &TargetKind,
    input_dir: &Path,
    requested: usize,
    seed: u64,
    entries: &[MutationManifestEntry],
) -> Result<PathBuf, String> {
    let entries_json = entries
        .iter()
        .map(|entry| {
            format!(
                "    {{\"index\": {}, \"source_seed\": \"{}\", \"output_path\": \"{}\", \"strategy\": \"{}\", \"mutation_seed\": {}, \"input_size\": {}, \"output_size\": {}, \"output_sha256\": \"{}\"}}",
                entry.index,
                json_escape(&entry.source_seed),
                json_escape(&entry.output_path),
                json_escape(entry.strategy),
                entry.mutation_seed,
                entry.input_size,
                entry.output_size,
                json_escape(&entry.output_sha256)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let body = format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"target\": \"{}\",\n  \"mode\": \"batch\",\n  \"input_dir\": \"{}\",\n  \"out_dir\": \"{}\",\n  \"requested\": {},\n  \"generated\": {},\n  \"seed\": {},\n  \"entries\": [\n{}\n  ]\n}}\n",
        json_escape(target_label(target)),
        json_escape(&input_dir.display().to_string()),
        json_escape(&out_dir.display().to_string()),
        requested,
        entries.len(),
        seed,
        entries_json
    );
    let manifest_path = out_dir.join("manifest.json");
    fs::write(&manifest_path, body).map_err(|e| {
        format!(
            "failed to write mutation manifest '{}': {e}",
            manifest_path.display()
        )
    })?;
    Ok(manifest_path)
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
