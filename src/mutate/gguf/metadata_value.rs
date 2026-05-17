use super::{parse_gguf, pick_different_ascii_byte, GgufValueType};
use crate::mutate::common::{DeterministicRng, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "metadata_value";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let layout = parse_gguf(bytes).map_err(|_| OperatorError::NoApplicableField)?;

    let candidates: Vec<usize> = (0..layout.kvs.len())
        .filter(|&i| {
            let kv = &layout.kvs[i];
            let has_payload = kv.value_payload_end > kv.value_payload_start;
            let suitable = !matches!(kv.value_type, GgufValueType::Array);
            has_payload && suitable
        })
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let kv_idx = candidates[rng.index(candidates.len())];
    let kv = &layout.kvs[kv_idx];

    let mut out = bytes.to_vec();
    let (byte_offset, original_byte, mutated_byte, kind_label) = match kv.value_type {
        GgufValueType::String => {
            let str_start = kv.value_payload_start + 8;
            let str_end = kv.value_payload_end;
            if str_end <= str_start {
                return Err(OperatorError::NoApplicableField);
            }
            let span = str_end - str_start;
            let pick = str_start + rng.index(span);
            let cur = out[pick];
            let new_b = pick_different_ascii_byte(rng, cur);
            out[pick] = new_b;
            (pick, cur, new_b, "string")
        }
        _ => {
            let pl_start = kv.value_payload_start;
            let pl_end = kv.value_payload_end;
            if pl_end <= pl_start {
                return Err(OperatorError::NoApplicableField);
            }
            let span = pl_end - pl_start;
            let pick = pl_start + rng.index(span);
            let cur = out[pick];
            let bit = rng.index(8) as u32;
            let new_b = cur ^ (1u8 << bit);
            out[pick] = new_b;
            (pick, cur, new_b, "scalar")
        }
    };

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("kv_index", kv_idx.to_string()),
            ("value_kind", kind_label.to_string()),
            ("byte_offset", byte_offset.to_string()),
            ("original_byte", format!("0x{:02x}", original_byte)),
            ("mutated_byte", format!("0x{:02x}", mutated_byte)),
        ],
        parse_preserving: "yes",
    })
}
