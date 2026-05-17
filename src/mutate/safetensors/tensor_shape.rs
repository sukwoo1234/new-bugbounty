use super::{digit_count_u64, parse_safetensors};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_shape";

fn parse_number_bytes(slice: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(slice).ok()?;
    s.parse::<u64>().ok()
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_safetensors(bytes).map_err(|_| OperatorError::NoApplicableField)?;

    let mut candidates: Vec<(usize, usize, u64, u64)> = Vec::new();
    for (t_idx, tensor) in layout.tensors.iter().enumerate() {
        for (dim_idx, elem) in tensor.shape.elements.iter().enumerate() {
            let raw = &bytes[elem.start..elem.end];
            if let Some(value) = parse_number_bytes(raw) {
                let mutated_plus = value.saturating_add(1);
                if digit_count_u64(mutated_plus) == digit_count_u64(value) {
                    candidates.push((t_idx, dim_idx, value, mutated_plus));
                }
                if value >= 1 {
                    let mutated_minus = value - 1;
                    if digit_count_u64(mutated_minus) == digit_count_u64(value) {
                        candidates.push((t_idx, dim_idx, value, mutated_minus));
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let pick_idx = rng.index(candidates.len());
    let (t_idx, dim_idx, original, mutated) = candidates[pick_idx];
    let span = &layout.tensors[t_idx].shape.elements[dim_idx];

    let mut out = bytes.to_vec();
    let new_str = mutated.to_string();
    let new_bytes = new_str.as_bytes();
    if new_bytes.len() != span.len() {
        return Err(OperatorError::NoApplicableField);
    }
    out[span.start..span.end].copy_from_slice(new_bytes);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("dim_index", dim_idx.to_string()),
            ("byte_offset", span.start.to_string()),
            ("original", original.to_string()),
            ("mutated", mutated.to_string()),
        ],
        parse_preserving: "no",
    })
}
