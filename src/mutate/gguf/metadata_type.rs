use super::{parse_gguf, write_u32};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "metadata_type";

const VALUE_TYPE_COUNT: u32 = 13;

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    if layout.kvs.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let kv_idx = rng.index(layout.kvs.len());
    let kv = &layout.kvs[kv_idx];

    let current = kv.value_type as u32;
    let new_type = loop {
        let candidate = rng.index(VALUE_TYPE_COUNT as usize) as u32;
        if candidate != current {
            break candidate;
        }
    };

    let mut out = bytes.to_vec();
    write_u32(&mut out, kv.value_type_start, new_type);

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("kv_index", kv_idx.to_string()),
            ("original_type", current.to_string()),
            ("mutated_type", new_type.to_string()),
        ],
        parse_preserving: "no",
    })
}
