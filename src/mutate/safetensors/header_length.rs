use super::{parse_safetensors, read_u64_le, write_u64_le};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "header_length";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    parse_safetensors(bytes).map_err(|_| OperatorError::NoApplicableField)?;

    let mut out = bytes.to_vec();
    let current = read_u64_le(&out, 0).map_err(|_| OperatorError::NoApplicableField)?;

    let delta_idx = rng.index(2);
    let (new_value, delta_label) = if delta_idx == 0 || current == 0 {
        (current.saturating_add(1), "+1")
    } else {
        (current - 1, "-1")
    };

    write_u64_le(&mut out, 0, new_value);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("delta", delta_label.to_string()),
            ("original", current.to_string()),
            ("mutated", new_value.to_string()),
        ],
        parse_preserving: "no",
    })
}
