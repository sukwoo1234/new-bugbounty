use std::collections::BTreeMap;

use super::{find_fields, DeterministicRng, FieldRef, MutationOutput, OperatorError};

pub(crate) const NAME: &str = "name_consistent";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut names: BTreeMap<Vec<u8>, Vec<FieldRef>> = BTreeMap::new();
    for path in [
        &[(7u32, 2u8), (1, 2), (1, 2)][..],
        &[(7, 2), (1, 2), (2, 2)][..],
        &[(7, 2), (11, 2), (1, 2)][..],
        &[(7, 2), (12, 2), (1, 2)][..],
        &[(7, 2), (13, 2), (1, 2)][..],
        &[(7, 2), (5, 2), (8, 2)][..],
    ] {
        for field in find_fields(bytes, path) {
            if field.value_end <= field.value_start {
                continue;
            }
            let value = bytes[field.value_start..field.value_end].to_vec();
            if value.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                names.entry(value).or_default().push(field);
            }
        }
    }

    let candidates: Vec<(Vec<u8>, Vec<FieldRef>)> = names
        .into_iter()
        .filter(|(name, refs)| name.len() >= 2 && refs.len() >= 2)
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }

    let (name, refs) = &candidates[rng.index(candidates.len())];
    let byte_index = rng.index(name.len());
    let bit = rng.index(6) as u32;
    let mask = 1u8 << bit;
    let mut out = bytes.to_vec();
    for field in refs {
        out[field.value_start + byte_index] ^= mask;
    }
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("occurrences", refs.len().to_string()),
            ("name_len", name.len().to_string()),
            ("byte_index", byte_index.to_string()),
            ("xor_mask", mask.to_string()),
            ("mutation_level", "2".to_string()),
        ],
        parse_preserving: "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_string_field};
    use super::*;

    fn fixture_with_shared_tensor_name() -> Vec<u8> {
        let mut producer = Vec::new();
        producer.extend(encode_string_field(2, "hidden"));
        let mut consumer = Vec::new();
        consumer.extend(encode_string_field(1, "hidden"));
        let mut graph = Vec::new();
        graph.extend(encode_length_delimited(1, &producer));
        graph.extend(encode_length_delimited(1, &consumer));
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn mutates_all_occurrences_of_shared_tensor_name() {
        let bytes = fixture_with_shared_tensor_name();
        let mut rng = DeterministicRng::new(11);
        let result = apply(&bytes, &mut rng).expect("shared name exists");
        assert_eq!(result.parse_preserving, "yes");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        assert_eq!(
            result
                .operator_params
                .iter()
                .find(|(key, _)| *key == "occurrences")
                .map(|(_, value)| value.as_str()),
            Some("2")
        );
    }
}
