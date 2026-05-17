use super::{parse_safetensors, pick_different_ascii_byte};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_name";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_safetensors(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    let candidates: Vec<usize> = (0..layout.tensors.len())
        .filter(|&i| layout.tensors[i].name.inner_len() > 0)
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let t_idx = candidates[rng.index(candidates.len())];
    let span = &layout.tensors[t_idx].name;
    let pick = span.inner_start + rng.index(span.inner_len());

    let mut out = bytes.to_vec();
    let original_byte = out[pick];
    let mutated_byte = pick_different_ascii_byte(rng, original_byte);
    out[pick] = mutated_byte;

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("byte_offset", pick.to_string()),
            ("original_byte", format!("0x{:02x}", original_byte)),
            ("mutated_byte", format!("0x{:02x}", mutated_byte)),
        ],
        parse_preserving: "yes",
    })
}
