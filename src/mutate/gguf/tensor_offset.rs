use super::{parse_gguf, read_u64, write_u64};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_offset";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    if layout.tensors.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let t_idx = rng.index(layout.tensors.len());
    let t = &layout.tensors[t_idx];

    let current = read_u64(bytes, t.offset_start).map_err(|_| OperatorError::NoApplicableField)?;
    let alignment = layout.alignment;

    let delta_idx = rng.index(2);
    let (new_offset, delta_label) = if delta_idx == 0 || current < alignment {
        (current.saturating_add(alignment), "+alignment")
    } else {
        (current - alignment, "-alignment")
    };

    let mut out = bytes.to_vec();
    write_u64(&mut out, t.offset_start, new_offset);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("alignment", alignment.to_string()),
            ("delta", delta_label.to_string()),
            ("original", current.to_string()),
            ("mutated", new_offset.to_string()),
        ],
        parse_preserving: "no",
    })
}
