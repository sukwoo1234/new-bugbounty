use super::parse_safetensors;
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "tensor_dtype";

const GROUP_2: &[&[u8]] = &[b"I8", b"U8"];
const GROUP_3: &[&[u8]] = &[
    b"F64", b"F32", b"F16", b"I64", b"I32", b"I16", b"U64", b"U32", b"U16",
];
const GROUP_4: &[&[u8]] = &[b"BF16", b"BOOL"];

fn group_for(len: usize) -> Option<&'static [&'static [u8]]> {
    match len {
        2 => Some(GROUP_2),
        3 => Some(GROUP_3),
        4 => Some(GROUP_4),
        _ => None,
    }
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_safetensors(bytes).map_err(|_| OperatorError::NoApplicableField)?;
    let candidates: Vec<usize> = (0..layout.tensors.len())
        .filter(|&i| {
            let span = &layout.tensors[i].dtype;
            let len = span.inner_len();
            let current = &bytes[span.inner_start..span.inner_end];
            match group_for(len) {
                Some(group) => group.iter().any(|d| *d != current),
                None => false,
            }
        })
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let t_idx = candidates[rng.index(candidates.len())];
    let span = &layout.tensors[t_idx].dtype;
    let current = &bytes[span.inner_start..span.inner_end];
    let group = group_for(span.inner_len()).ok_or(OperatorError::NoApplicableField)?;
    let alternatives: Vec<&&[u8]> = group.iter().filter(|d| **d != current).collect();
    let pick = alternatives[rng.index(alternatives.len())];
    let new_dtype = *pick;

    let mut out = bytes.to_vec();
    out[span.inner_start..span.inner_end].copy_from_slice(new_dtype);

    let original = std::str::from_utf8(current).unwrap_or("?").to_string();
    let mutated = std::str::from_utf8(new_dtype).unwrap_or("?").to_string();

    let parse_preserving = super::parse_preserving_label(&out);
    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("tensor_index", t_idx.to_string()),
            ("byte_offset", span.inner_start.to_string()),
            ("original", original),
            ("mutated", mutated),
        ],
        parse_preserving,
    })
}
