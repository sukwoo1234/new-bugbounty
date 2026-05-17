use super::{parse_gguf, pick_different_ascii_byte, truncate_param_str};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_name";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    let candidates: Vec<usize> = (0..layout.tensors.len())
        .filter(|&i| layout.tensors[i].name_str_end > layout.tensors[i].name_str_start)
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let t_idx = candidates[rng.index(candidates.len())];
    let t = &layout.tensors[t_idx];

    let original_name = std::str::from_utf8(&bytes[t.name_str_start..t.name_str_end])
        .unwrap_or("not_available")
        .to_string();

    let mut out = bytes.to_vec();
    let span = t.name_str_end - t.name_str_start;
    let pick = t.name_str_start + rng.index(span);
    let cur = out[pick];
    let new_b = pick_different_ascii_byte(rng, cur);
    out[pick] = new_b;

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("original_name", truncate_param_str(&original_name, 64)),
            ("byte_offset", pick.to_string()),
        ],
        parse_preserving: "yes",
    })
}
