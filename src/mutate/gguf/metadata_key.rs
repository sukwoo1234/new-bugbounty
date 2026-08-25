use super::{parse_gguf, pick_different_ascii_byte, truncate_param_str};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "metadata_key";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;

    let candidates: Vec<usize> = (0..layout.kvs.len())
        .filter(|&i| layout.kvs[i].key_str_end > layout.kvs[i].key_str_start)
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let kv_idx = candidates[rng.index(candidates.len())];
    let kv = &layout.kvs[kv_idx];

    let original_key = std::str::from_utf8(&bytes[kv.key_str_start..kv.key_str_end])
        .unwrap_or("not_available")
        .to_string();

    let mut out = bytes.to_vec();
    let span = kv.key_str_end - kv.key_str_start;
    let pick = kv.key_str_start + rng.index(span);
    let cur = out[pick];
    let new_b = pick_different_ascii_byte(rng, cur);
    out[pick] = new_b;

    let parse_preserving = parse_preserving_label(&out);
    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("kv_index", kv_idx.to_string()),
            ("original_key", truncate_param_str(&original_key, 64)),
            ("byte_offset", pick.to_string()),
        ],
        parse_preserving,
    })
}

/// Whether the mutated bytes still parse.
///
/// A17/A18: these operators substitute an arbitrary printable byte into a string,
/// which can be a quote or backslash in a safetensors JSON header, or the middle
/// of a multi-byte UTF-8 sequence in GGUF. The output was labelled
/// parse_preserving="yes" regardless, so the manifest asserted something the file
/// no longer satisfied. The mutation itself is worth keeping - a parser's handling
/// of its own delimiters is exactly what wants exercising - so the label is
/// derived instead of claimed.
fn parse_preserving_label(bytes: &[u8]) -> &'static str {
    if parse_gguf(bytes).is_ok() {
        "yes"
    } else {
        "no"
    }
}
