use super::{parse_gguf, read_u64, write_u64};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_shape";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    let candidates: Vec<usize> = (0..layout.tensors.len())
        .filter(|&i| layout.tensors[i].n_dims > 0)
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let t_idx = candidates[rng.index(candidates.len())];
    let t = &layout.tensors[t_idx];

    let dim_idx = rng.index(t.n_dims as usize);
    let dim_offset = t.shape_start + dim_idx * 8;
    let current = read_u64(bytes, dim_offset).map_err(|_| OperatorError::NoApplicableField)?;

    let delta_idx = rng.index(2);
    let (new_value, delta_label) = if delta_idx == 0 || current == 0 {
        (current.saturating_add(1), "+1")
    } else {
        (current - 1, "-1")
    };

    let mut out = bytes.to_vec();
    write_u64(&mut out, dim_offset, new_value);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("dim_index", dim_idx.to_string()),
            ("n_dims", t.n_dims.to_string()),
            ("delta", delta_label.to_string()),
            ("original", current.to_string()),
            ("mutated", new_value.to_string()),
        ],
        parse_preserving: "no",
    })
}
