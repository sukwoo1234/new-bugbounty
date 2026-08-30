use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::mutate::common::{
    operator_set, print_batch_mutation_report, print_mutation_report, select_operator,
    single_manifest_path, write_mutation_manifest, write_output, BatchMutationReport,
    DeterministicRng, MutationManifestEntry, MutationOutput, MutationReport, OperatorError,
};
use crate::{common::sha256_file, target::TargetKind};

pub(crate) mod byte_flip;
pub(crate) mod header_counts;
pub(crate) mod kv_insert;
pub(crate) mod metadata_key;
pub(crate) mod metadata_type;
pub(crate) mod metadata_value;
pub(crate) mod tensor_dtype;
pub(crate) mod tensor_name;
pub(crate) mod tensor_offset;
pub(crate) mod tensor_shape;

pub(crate) const MAGIC: &[u8; 4] = b"GGUF";
pub(crate) const SUPPORTED_VERSION: u32 = 3;
pub(crate) const DEFAULT_ALIGNMENT: u64 = 32;
pub(crate) const ALIGNMENT_KEY: &str = "general.alignment";

pub(crate) const DEFAULT_OPERATORS: &[&str] = &[
    header_counts::NAME,
    metadata_value::NAME,
    metadata_key::NAME,
    tensor_name::NAME,
    tensor_dtype::NAME,
    tensor_shape::NAME,
    tensor_offset::NAME,
];

pub(crate) const KNOWN_OPERATORS: &[&str] = &[
    header_counts::NAME,
    metadata_value::NAME,
    metadata_key::NAME,
    tensor_name::NAME,
    tensor_dtype::NAME,
    tensor_shape::NAME,
    tensor_offset::NAME,
    byte_flip::NAME,
    metadata_type::NAME,
    kv_insert::NAME,
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
        "header_counts" => header_counts::apply(bytes, rng),
        "metadata_value" => metadata_value::apply(bytes, rng),
        "metadata_key" => metadata_key::apply(bytes, rng),
        "tensor_name" => tensor_name::apply(bytes, rng),
        "tensor_dtype" => tensor_dtype::apply(bytes, rng),
        "tensor_shape" => tensor_shape::apply(bytes, rng),
        "tensor_offset" => tensor_offset::apply(bytes, rng),
        "byte_flip" => byte_flip::apply(bytes, rng),
        "metadata_type" => metadata_type::apply(bytes, rng),
        "kv_insert" => kv_insert::apply(bytes, rng),
        _ => Err(OperatorError::NoApplicableField),
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
        "tool/mutate/gguf",
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

    let inputs = collect_gguf_inputs(input_dir)?;
    if inputs.is_empty() {
        return Err(format!(
            "no .gguf input files found in {}",
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
        let out = out_dir.join(format!("mut-gguf-{:06}.gguf", idx + 1));
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
        "tool/mutate/gguf",
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

fn collect_gguf_inputs(input_dir: &Path) -> Result<Vec<PathBuf>, String> {
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
                .map(|e| e.eq_ignore_ascii_case("gguf"))
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
    use super::{align_up, GgufValueType};

    pub(crate) fn build_minimal_gguf() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&1u64.to_le_bytes());
        buf.extend_from_slice(&2u64.to_le_bytes());

        let key1 = b"general.name";
        buf.extend_from_slice(&(key1.len() as u64).to_le_bytes());
        buf.extend_from_slice(key1);
        buf.extend_from_slice(&(GgufValueType::String as u32).to_le_bytes());
        let val1 = b"test";
        buf.extend_from_slice(&(val1.len() as u64).to_le_bytes());
        buf.extend_from_slice(val1);

        let key2 = b"general.alignment";
        buf.extend_from_slice(&(key2.len() as u64).to_le_bytes());
        buf.extend_from_slice(key2);
        buf.extend_from_slice(&(GgufValueType::U32 as u32).to_le_bytes());
        buf.extend_from_slice(&32u32.to_le_bytes());

        let tname = b"w0";
        buf.extend_from_slice(&(tname.len() as u64).to_le_bytes());
        buf.extend_from_slice(tname);
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&3u64.to_le_bytes());
        buf.extend_from_slice(&4u64.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());

        let pad = align_up(buf.len(), 32) - buf.len();
        buf.extend(std::iter::repeat(0u8).take(pad));
        buf.extend(std::iter::repeat(0u8).take(48));
        buf
    }


    /// A fixture with NO `general.alignment` key. This is the realistic shape: none of
    /// the 18 gguf seeds carries that key, and ggml rejects a file that carries it
    /// twice (gguf.cpp:431), so an insert operator has to start from a file without it.
    pub(crate) fn build_gguf_without_alignment_key() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&1u64.to_le_bytes());
        buf.extend_from_slice(&1u64.to_le_bytes());

        let key = b"general.architecture";
        buf.extend_from_slice(&(key.len() as u64).to_le_bytes());
        buf.extend_from_slice(key);
        buf.extend_from_slice(&(GgufValueType::String as u32).to_le_bytes());
        let val = b"llama";
        buf.extend_from_slice(&(val.len() as u64).to_le_bytes());
        buf.extend_from_slice(val);

        let tname = b"w0";
        buf.extend_from_slice(&(tname.len() as u64).to_le_bytes());
        buf.extend_from_slice(tname);
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&3u64.to_le_bytes());
        buf.extend_from_slice(&4u64.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());

        buf.resize(align_up(buf.len(), 32), 0);
        for i in 0..48u8 {
            buf.push(i.wrapping_mul(7).wrapping_add(1));
        }
        buf
    }

    /// A fixture whose tensor name is not pure ASCII. Real models carry these, and
    /// a one-byte substitution inside a multi-byte sequence breaks the UTF-8 the
    /// GGUF format requires.
    pub(crate) fn build_gguf_with_multibyte_tensor_name() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&1u64.to_le_bytes());
        buf.extend_from_slice(&1u64.to_le_bytes());

        let key = b"general.alignment";
        buf.extend_from_slice(&(key.len() as u64).to_le_bytes());
        buf.extend_from_slice(key);
        buf.extend_from_slice(&(GgufValueType::U32 as u32).to_le_bytes());
        buf.extend_from_slice(&32u32.to_le_bytes());

        let tname = "가중치".as_bytes();
        buf.extend_from_slice(&(tname.len() as u64).to_le_bytes());
        buf.extend_from_slice(tname);
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&3u64.to_le_bytes());
        buf.extend_from_slice(&4u64.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());

        buf.resize(align_up(buf.len(), 32), 0);
        buf.resize(buf.len() + 48, 0);
        buf
    }

    /// A fixture whose metadata holds an EMPTY string value beside a scalar. A real
    /// model has these - an unset `general.description`, say.
    pub(crate) fn build_gguf_with_empty_string_value() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&2u64.to_le_bytes());

        let key1 = b"general.description";
        buf.extend_from_slice(&(key1.len() as u64).to_le_bytes());
        buf.extend_from_slice(key1);
        buf.extend_from_slice(&(GgufValueType::String as u32).to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());

        let key2 = b"general.alignment";
        buf.extend_from_slice(&(key2.len() as u64).to_le_bytes());
        buf.extend_from_slice(key2);
        buf.extend_from_slice(&(GgufValueType::U32 as u32).to_le_bytes());
        buf.extend_from_slice(&32u32.to_le_bytes());

        buf.resize(align_up(buf.len(), 32), 0);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::build_minimal_gguf;
    use super::*;

    #[test]
    fn skip_value_rejects_deeply_nested_arrays() {
        // A5 regression: an Array-of-Array chain with no depth limit drives unbounded
        // recursion (stack overflow / SIGABRT). parse_gguf calls skip_value for every
        // metadata value, so a crafted seed must yield a clean parse error, not a crash.
        let levels = 500usize;
        let mut payload = Vec::new();
        for i in 0..levels {
            if i + 1 < levels {
                payload.extend_from_slice(&(GgufValueType::Array as u32).to_le_bytes());
                payload.extend_from_slice(&1u64.to_le_bytes());
            } else {
                // innermost: an empty scalar array terminates the chain.
                payload.extend_from_slice(&(GgufValueType::U8 as u32).to_le_bytes());
                payload.extend_from_slice(&0u64.to_le_bytes());
            }
        }
        let result = skip_value(&payload, 0, GgufValueType::Array);
        assert!(
            matches!(result, Err(ParseError::NestingTooDeep)),
            "deeply nested arrays must be rejected with NestingTooDeep"
        );
    }

    #[test]
    fn parse_minimal_gguf_succeeds() {
        let bytes = build_minimal_gguf();
        let layout = parse_gguf(&bytes).expect("minimal fixture must parse");
        assert_eq!(layout.version, 3);
        assert_eq!(layout.tensor_count, 1);
        assert_eq!(layout.kv_count, 2);
        assert_eq!(layout.alignment, 32);
        assert_eq!(layout.kvs.len(), 2);
        assert_eq!(layout.tensors.len(), 1);
        assert_eq!(layout.tensor_data_start % 32, 0);
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let mut bytes = build_minimal_gguf();
        bytes[0] = b'X';
        assert!(matches!(parse_gguf(&bytes), Err(ParseError::BadMagic)));
    }

    #[test]
    fn parse_rejects_too_small() {
        assert!(matches!(parse_gguf(&[0u8; 8]), Err(ParseError::TooSmall)));
    }

    #[test]
    fn parse_rejects_v2() {
        let mut bytes = build_minimal_gguf();
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(matches!(
            parse_gguf(&bytes),
            Err(ParseError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn validate_operators_accepts_known() {
        let req = vec!["metadata_value".to_string(), "byte_flip".to_string()];
        let resolved = validate_operators(&req).expect("known operators accepted");
        assert_eq!(resolved, vec!["metadata_value", "byte_flip"]);
    }

    #[test]
    fn validate_operators_rejects_unknown() {
        let req = vec!["does_not_exist".to_string()];
        assert!(validate_operators(&req).is_err());
    }

    #[test]
    fn default_operators_excludes_opt_in() {
        assert!(!DEFAULT_OPERATORS.contains(&byte_flip::NAME));
        assert!(!DEFAULT_OPERATORS.contains(&metadata_type::NAME));
        assert!(KNOWN_OPERATORS.contains(&byte_flip::NAME));
        assert!(KNOWN_OPERATORS.contains(&metadata_type::NAME));
    }

    #[test]
    fn header_counts_mutates_one_count() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(7);
        let result = header_counts::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        let new_tc = u64::from_le_bytes(result.bytes[8..16].try_into().unwrap());
        let new_kc = u64::from_le_bytes(result.bytes[16..24].try_into().unwrap());
        assert!(new_tc != 1 || new_kc != 2);
        assert_eq!(result.parse_preserving, "no");
    }

    #[test]
    fn metadata_value_mutates_payload() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(11);
        let result = metadata_value::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
        assert_eq!(result.parse_preserving, "yes");
    }

    #[test]
    fn metadata_key_preserves_length() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(13);
        let result = metadata_key::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
        assert_eq!(result.parse_preserving, "yes");
    }

    #[test]
    fn tensor_name_preserves_length() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(17);
        let result = tensor_name::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
        assert_eq!(result.parse_preserving, "yes");
    }

    #[test]
    fn tensor_dtype_changes_type_field() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(19);
        let result = tensor_dtype::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.parse_preserving, "no");
    }

    #[test]
    fn tensor_shape_preserves_n_dims() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(23);
        let result = tensor_shape::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        let layout_before = parse_gguf(&bytes).expect("parse before");
        let layout_after = parse_gguf(&result.bytes).expect("parse after");
        assert_eq!(layout_before.tensors[0].n_dims, layout_after.tensors[0].n_dims);
    }

    #[test]
    fn tensor_offset_preserves_alignment() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(29);
        let layout = parse_gguf(&bytes).expect("parse");
        let alignment = layout.alignment;
        let result = tensor_offset::apply(&bytes, &mut rng).expect("apply");
        let new_offset_arr: [u8; 8] = result.bytes[layout.tensors[0].offset_start
            ..layout.tensors[0].offset_start + 8]
            .try_into()
            .unwrap();
        let new_offset = u64::from_le_bytes(new_offset_arr);
        assert_eq!(new_offset % alignment, 0);
        assert_eq!(result.parse_preserving, "no");
    }

    #[test]
    fn byte_flip_targets_header_or_metadata_region() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(31);
        let layout = parse_gguf(&bytes).expect("parse");
        let result = byte_flip::apply(&bytes, &mut rng).expect("apply");
        assert_ne!(result.bytes, bytes);
        let diff_idx = (0..bytes.len())
            .find(|&i| bytes[i] != result.bytes[i])
            .expect("difference exists");
        assert!(diff_idx < layout.tensor_data_start);
    }

    #[test]
    fn metadata_type_changes_value_type_enum() {
        let bytes = build_minimal_gguf();
        let mut rng = DeterministicRng::new(37);
        let layout = parse_gguf(&bytes).expect("parse");
        let kv0_type_start = layout.kvs[0].value_type_start;
        let result = metadata_type::apply(&bytes, &mut rng).expect("apply");
        let old_t = u32::from_le_bytes(
            bytes[kv0_type_start..kv0_type_start + 4]
                .try_into()
                .unwrap(),
        );
        let new_t_kv0 = u32::from_le_bytes(
            result.bytes[kv0_type_start..kv0_type_start + 4]
                .try_into()
                .unwrap(),
        );
        let kv1_type_start = layout.kvs[1].value_type_start;
        let old_t1 = u32::from_le_bytes(
            bytes[kv1_type_start..kv1_type_start + 4]
                .try_into()
                .unwrap(),
        );
        let new_t1 = u32::from_le_bytes(
            result.bytes[kv1_type_start..kv1_type_start + 4]
                .try_into()
                .unwrap(),
        );
        assert!(new_t_kv0 != old_t || new_t1 != old_t1);
        assert_eq!(result.parse_preserving, "no");
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    TooSmall,
    BadMagic,
    UnsupportedVersion(u32),
    Truncated(&'static str),
    InvalidValueType(u32),
    OversizedCount(&'static str),
    InvalidUtf8,
    NestingTooDeep,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall => write!(f, "gguf input smaller than 24-byte header"),
            Self::BadMagic => write!(f, "gguf magic mismatch (expected 'GGUF')"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported gguf version {} (supported: 3)", v)
            }
            Self::Truncated(where_) => write!(f, "gguf input truncated at {}", where_),
            Self::InvalidValueType(v) => write!(f, "gguf invalid value_type {}", v),
            Self::OversizedCount(where_) => write!(f, "gguf oversized count at {}", where_),
            Self::InvalidUtf8 => write!(f, "gguf string is not valid utf-8"),
            Self::NestingTooDeep => write!(f, "gguf array nesting exceeds depth limit"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GgufValueType {
    U8 = 0,
    I8 = 1,
    U16 = 2,
    I16 = 3,
    U32 = 4,
    I32 = 5,
    F32 = 6,
    Bool = 7,
    String = 8,
    Array = 9,
    U64 = 10,
    I64 = 11,
    F64 = 12,
}

impl GgufValueType {
    pub(crate) fn from_u32(n: u32) -> Option<Self> {
        match n {
            0 => Some(Self::U8),
            1 => Some(Self::I8),
            2 => Some(Self::U16),
            3 => Some(Self::I16),
            4 => Some(Self::U32),
            5 => Some(Self::I32),
            6 => Some(Self::F32),
            7 => Some(Self::Bool),
            8 => Some(Self::String),
            9 => Some(Self::Array),
            10 => Some(Self::U64),
            11 => Some(Self::I64),
            12 => Some(Self::F64),
            _ => None,
        }
    }

    pub(crate) fn scalar_size(self) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 | Self::Bool => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 | Self::F32 => Some(4),
            Self::U64 | Self::I64 | Self::F64 => Some(8),
            Self::String | Self::Array => None,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct KvEntry {
    pub(crate) entry_start: usize,
    pub(crate) entry_end: usize,
    pub(crate) key_str_start: usize,
    pub(crate) key_str_end: usize,
    pub(crate) value_type: GgufValueType,
    pub(crate) value_type_start: usize,
    pub(crate) value_payload_start: usize,
    pub(crate) value_payload_end: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TensorEntry {
    pub(crate) entry_start: usize,
    pub(crate) entry_end: usize,
    pub(crate) name_str_start: usize,
    pub(crate) name_str_end: usize,
    pub(crate) n_dims: u32,
    pub(crate) shape_start: usize,
    pub(crate) shape_end: usize,
    pub(crate) type_start: usize,
    pub(crate) offset_start: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct GgufLayout {
    pub(crate) version: u32,
    pub(crate) tensor_count: u64,
    pub(crate) kv_count: u64,
    pub(crate) alignment: u64,
    pub(crate) kvs: Vec<KvEntry>,
    pub(crate) tensors: Vec<TensorEntry>,
    pub(crate) tensor_data_start: usize,
}

pub(crate) fn parse_gguf(bytes: &[u8]) -> Result<GgufLayout, ParseError> {
    if bytes.len() < 24 {
        return Err(ParseError::TooSmall);
    }
    if &bytes[0..4] != MAGIC {
        return Err(ParseError::BadMagic);
    }
    let version = read_u32(bytes, 4)?;
    if version != SUPPORTED_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let tensor_count = read_u64(bytes, 8)?;
    let kv_count = read_u64(bytes, 16)?;

    let mut cursor = 24usize;
    let mut kvs = Vec::new();
    let mut alignment = DEFAULT_ALIGNMENT;
    for _ in 0..kv_count {
        let entry_start = cursor;
        let key_len = read_u64(bytes, cursor)?;
        cursor = cursor
            .checked_add(8)
            .ok_or(ParseError::Truncated("kv:key_len"))?;
        let key_len_usize = usize::try_from(key_len)
            .map_err(|_| ParseError::OversizedCount("kv:key_len"))?;
        let key_str_start = cursor;
        let key_str_end = cursor
            .checked_add(key_len_usize)
            .ok_or(ParseError::Truncated("kv:key_str"))?;
        if key_str_end > bytes.len() {
            return Err(ParseError::Truncated("kv:key_str"));
        }
        std::str::from_utf8(&bytes[key_str_start..key_str_end])
            .map_err(|_| ParseError::InvalidUtf8)?;
        cursor = key_str_end;
        let value_type_start = cursor;
        let value_type_raw = read_u32(bytes, cursor)?;
        let value_type = GgufValueType::from_u32(value_type_raw)
            .ok_or(ParseError::InvalidValueType(value_type_raw))?;
        cursor = cursor
            .checked_add(4)
            .ok_or(ParseError::Truncated("kv:value_type"))?;
        let value_payload_start = cursor;
        let value_payload_end = skip_value(bytes, cursor, value_type)?;
        cursor = value_payload_end;

        if value_type == GgufValueType::U32
            && &bytes[key_str_start..key_str_end] == ALIGNMENT_KEY.as_bytes()
        {
            let raw = read_u32(bytes, value_payload_start)? as u64;
            if raw > 0 {
                alignment = raw;
            }
        }

        kvs.push(KvEntry {
            entry_start,
            entry_end: value_payload_end,
            key_str_start,
            key_str_end,
            value_type,
            value_type_start,
            value_payload_start,
            value_payload_end,
        });
    }

    let mut tensors = Vec::new();
    for _ in 0..tensor_count {
        let entry_start = cursor;
        let name_len = read_u64(bytes, cursor)?;
        cursor = cursor
            .checked_add(8)
            .ok_or(ParseError::Truncated("tensor:name_len"))?;
        let name_len_usize = usize::try_from(name_len)
            .map_err(|_| ParseError::OversizedCount("tensor:name_len"))?;
        let name_str_start = cursor;
        let name_str_end = cursor
            .checked_add(name_len_usize)
            .ok_or(ParseError::Truncated("tensor:name_str"))?;
        if name_str_end > bytes.len() {
            return Err(ParseError::Truncated("tensor:name_str"));
        }
        std::str::from_utf8(&bytes[name_str_start..name_str_end])
            .map_err(|_| ParseError::InvalidUtf8)?;
        cursor = name_str_end;
        let n_dims = read_u32(bytes, cursor)?;
        cursor = cursor
            .checked_add(4)
            .ok_or(ParseError::Truncated("tensor:n_dims"))?;
        let shape_start = cursor;
        let shape_byte_len = (n_dims as usize)
            .checked_mul(8)
            .ok_or(ParseError::OversizedCount("tensor:shape_len"))?;
        let shape_end = cursor
            .checked_add(shape_byte_len)
            .ok_or(ParseError::Truncated("tensor:shape"))?;
        if shape_end > bytes.len() {
            return Err(ParseError::Truncated("tensor:shape"));
        }
        cursor = shape_end;
        let type_start = cursor;
        cursor = cursor
            .checked_add(4)
            .ok_or(ParseError::Truncated("tensor:type"))?;
        if cursor > bytes.len() {
            return Err(ParseError::Truncated("tensor:type"));
        }
        let offset_start = cursor;
        cursor = cursor
            .checked_add(8)
            .ok_or(ParseError::Truncated("tensor:offset"))?;
        if cursor > bytes.len() {
            return Err(ParseError::Truncated("tensor:offset"));
        }
        tensors.push(TensorEntry {
            entry_start,
            entry_end: cursor,
            name_str_start,
            name_str_end,
            n_dims,
            shape_start,
            shape_end,
            type_start,
            offset_start,
        });
    }

    let tensor_data_start = align_up(cursor, alignment as usize);

    Ok(GgufLayout {
        version,
        tensor_count,
        kv_count,
        alignment,
        kvs,
        tensors,
        tensor_data_start,
    })
}

// GGUF metadata arrays are flat in practice; a small cap prevents a crafted
// Array-of-Array chain from exhausting the stack during parse (A5).
const MAX_VALUE_NESTING_DEPTH: usize = 64;

fn skip_value(bytes: &[u8], start: usize, value_type: GgufValueType) -> Result<usize, ParseError> {
    skip_value_depth(bytes, start, value_type, 0)
}

fn skip_value_depth(
    bytes: &[u8],
    start: usize,
    value_type: GgufValueType,
    depth: usize,
) -> Result<usize, ParseError> {
    if depth > MAX_VALUE_NESTING_DEPTH {
        return Err(ParseError::NestingTooDeep);
    }
    if let Some(scalar) = value_type.scalar_size() {
        let end = start
            .checked_add(scalar)
            .ok_or(ParseError::Truncated("value:scalar"))?;
        if end > bytes.len() {
            return Err(ParseError::Truncated("value:scalar"));
        }
        return Ok(end);
    }
    match value_type {
        GgufValueType::String => {
            let len = read_u64(bytes, start)?;
            let after_len = start
                .checked_add(8)
                .ok_or(ParseError::Truncated("value:string_len"))?;
            let len_usize = usize::try_from(len)
                .map_err(|_| ParseError::OversizedCount("value:string_len"))?;
            let end = after_len
                .checked_add(len_usize)
                .ok_or(ParseError::Truncated("value:string"))?;
            if end > bytes.len() {
                return Err(ParseError::Truncated("value:string"));
            }
            std::str::from_utf8(&bytes[after_len..end]).map_err(|_| ParseError::InvalidUtf8)?;
            Ok(end)
        }
        GgufValueType::Array => {
            let elem_type_raw = read_u32(bytes, start)?;
            let elem_type = GgufValueType::from_u32(elem_type_raw)
                .ok_or(ParseError::InvalidValueType(elem_type_raw))?;
            let after_elem_type = start
                .checked_add(4)
                .ok_or(ParseError::Truncated("value:array_elem_type"))?;
            let count = read_u64(bytes, after_elem_type)?;
            let after_count = after_elem_type
                .checked_add(8)
                .ok_or(ParseError::Truncated("value:array_count"))?;
            let count_usize = usize::try_from(count)
                .map_err(|_| ParseError::OversizedCount("value:array_count"))?;
            let mut cursor = after_count;
            for _ in 0..count_usize {
                cursor = skip_value_depth(bytes, cursor, elem_type, depth + 1)?;
            }
            Ok(cursor)
        }
        _ => Err(ParseError::InvalidValueType(value_type as u32)),
    }
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ParseError> {
    let end = offset.checked_add(4).ok_or(ParseError::Truncated("u32"))?;
    if end > bytes.len() {
        return Err(ParseError::Truncated("u32"));
    }
    let mut arr = [0u8; 4];
    arr.copy_from_slice(&bytes[offset..end]);
    Ok(u32::from_le_bytes(arr))
}

pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ParseError> {
    let end = offset.checked_add(8).ok_or(ParseError::Truncated("u64"))?;
    if end > bytes.len() {
        return Err(ParseError::Truncated("u64"));
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[offset..end]);
    Ok(u64::from_le_bytes(arr))
}

pub(crate) fn write_u32(out: &mut [u8], offset: usize, value: u32) {
    let bytes = value.to_le_bytes();
    out[offset..offset + 4].copy_from_slice(&bytes);
}

pub(crate) fn write_u64(out: &mut [u8], offset: usize, value: u64) {
    let bytes = value.to_le_bytes();
    out[offset..offset + 8].copy_from_slice(&bytes);
}

pub(crate) fn pick_different_ascii_byte(rng: &mut DeterministicRng, current: u8) -> u8 {
    let range = (0x7eu8 - 0x21u8 + 1) as usize;
    loop {
        let candidate = 0x21u8 + (rng.index(range) as u8);
        if candidate != current {
            return candidate;
        }
    }
}

pub(crate) fn truncate_param_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

pub(crate) fn align_up(value: usize, alignment: usize) -> usize {
    if alignment <= 1 {
        return value;
    }
    let rem = value % alignment;
    if rem == 0 {
        value
    } else {
        value + (alignment - rem)
    }
}
