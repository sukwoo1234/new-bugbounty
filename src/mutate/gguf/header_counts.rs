use super::{parse_gguf, write_u64};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "header_counts";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;

    let mut out = bytes.to_vec();

    let field_idx = rng.index(2);
    let (offset, field_name) = if field_idx == 0 {
        (8usize, "tensor_count")
    } else {
        (16usize, "kv_count")
    };

    let mut arr = [0u8; 8];
    arr.copy_from_slice(&out[offset..offset + 8]);
    let current = u64::from_le_bytes(arr);

    let delta_idx = rng.index(2);
    let (new_value, delta_label) = if delta_idx == 0 || current == 0 {
        (current.saturating_add(1), "+1")
    } else {
        (current - 1, "-1")
    };

    write_u64(&mut out, offset, new_value);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("field", field_name.to_string()),
            ("delta", delta_label.to_string()),
            ("original", current.to_string()),
            ("mutated", new_value.to_string()),
        ],
        parse_preserving: "no",
    })
}
