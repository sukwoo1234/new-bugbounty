use std::collections::BTreeMap;

use super::{
    find_fields, read_varint_value, write_varint_same_width, DeterministicRng, FieldRef,
    MutationOutput, OperatorError,
};

pub(crate) const NAME: &str = "dtype_valid";

const FLOAT_DTYPES: &[u64] = &[1, 10, 11];

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut by_dtype: BTreeMap<u64, Vec<FieldRef>> = BTreeMap::new();
    for path in [
        &[(7u32, 2u8), (13, 2), (2, 2), (1, 2), (1, 0)][..],
        &[(7, 2), (11, 2), (2, 2), (1, 2), (1, 0)][..],
        &[(7, 2), (12, 2), (2, 2), (1, 2), (1, 0)][..],
    ] {
        for field in find_fields(bytes, path) {
            let Some(dtype) = read_varint_value(bytes, field.value_start, field.value_end) else {
                continue;
            };
            if FLOAT_DTYPES.contains(&dtype) {
                by_dtype.entry(dtype).or_default().push(field);
            }
        }
    }

    let candidates: Vec<(u64, Vec<FieldRef>)> = by_dtype
        .into_iter()
        .filter(|(_, fields)| fields.len() >= 2)
        .collect();
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }

    let (original, fields) = &candidates[rng.index(candidates.len())];
    let mut new_dtype = FLOAT_DTYPES[rng.index(FLOAT_DTYPES.len())];
    if new_dtype == *original {
        new_dtype = FLOAT_DTYPES
            .iter()
            .copied()
            .find(|value| *value != *original)
            .ok_or(OperatorError::NoApplicableField)?;
    }

    let mut out = bytes.to_vec();
    for field in fields {
        write_varint_same_width(&mut out, field.value_start, field.value_end, new_dtype)
            .ok_or(OperatorError::NoApplicableField)?;
    }
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("occurrences", fields.len().to_string()),
            ("old_dtype", (*original).to_string()),
            ("new_dtype", new_dtype.to_string()),
            ("mutation_level", "2".to_string()),
        ],
        parse_preserving: "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_varint_field};
    use super::*;

    fn fixture_with_repeated_dtype() -> Vec<u8> {
        let mut tensor_type = Vec::new();
        tensor_type.extend(encode_varint_field(1, 1));
        let type_proto = encode_length_delimited(1, &tensor_type);
        let value_info = encode_length_delimited(2, &type_proto);
        let mut graph = Vec::new();
        graph.extend(encode_length_delimited(11, &value_info));
        graph.extend(encode_length_delimited(13, &value_info));
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn writes_repeated_float_dtype_without_changing_size() {
        let bytes = fixture_with_repeated_dtype();
        let mut rng = DeterministicRng::new(9);
        let result = apply(&bytes, &mut rng).expect("dtype exists");
        assert_eq!(result.parse_preserving, "yes");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        for path in [
            &[(7u32, 2u8), (11, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (13, 2), (2, 2), (1, 2), (1, 0)][..],
        ] {
            let after = find_fields(&result.bytes, path);
            let value = read_varint_value(&result.bytes, after[0].value_start, after[0].value_end)
                .expect("dtype remains readable");
            assert!(FLOAT_DTYPES.contains(&value));
            assert_ne!(value, 1);
        }
    }
}
