use super::{parse_gguf, read_u32, write_u32};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_dtype";

const GGML_TYPE_CANDIDATES: u32 = 30;

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

    let current = read_u32(bytes, t.type_start).map_err(|_| OperatorError::NoApplicableField)?;

    let new_type = loop {
        let candidate = rng.index(GGML_TYPE_CANDIDATES as usize) as u32;
        if candidate != current {
            break candidate;
        }
    };

    let mut out = bytes.to_vec();
    write_u32(&mut out, t.type_start, new_type);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("original_type", current.to_string()),
            ("mutated_type", new_type.to_string()),
        ],
        parse_preserving: "no",
    })
}
