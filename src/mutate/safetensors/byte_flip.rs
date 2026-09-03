use super::parse_safetensors;
use crate::mutate::common::{flip_value_byte, DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "byte_flip";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_safetensors(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    let end = layout.header_end.min(bytes.len());
    if end == 0 {
        return Err(OperatorError::NoApplicableField);
    }
    let mut out = bytes.to_vec();
    let pick = flip_value_byte(&mut out, 0, end, rng).ok_or(OperatorError::NoApplicableField)?;
    let parse_preserving = super::parse_preserving_label(&out);
    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("byte_offset", pick.to_string()),
            ("region", "header".to_string()),
            ("region_end", end.to_string()),
        ],
        parse_preserving,
    })
}
