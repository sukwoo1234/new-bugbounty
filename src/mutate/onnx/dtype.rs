use super::{
    find_fields, flip_varint_byte, pick_field, DeterministicRng, MutationOutput, OperatorError,
};

pub(crate) const NAME: &str = "dtype";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut candidates = Vec::new();
    candidates.extend(find_fields(bytes, &[(7, 2), (5, 2), (2, 0)]));
    candidates.extend(find_fields(
        bytes,
        &[(7, 2), (13, 2), (2, 2), (1, 2), (1, 0)],
    ));
    candidates.extend(find_fields(
        bytes,
        &[(7, 2), (11, 2), (2, 2), (1, 2), (1, 0)],
    ));
    candidates.extend(find_fields(
        bytes,
        &[(7, 2), (12, 2), (2, 2), (1, 2), (1, 0)],
    ));

    let field = pick_field(&candidates, rng)
        .copied()
        .ok_or(OperatorError::NoApplicableField)?;
    let mut out = bytes.to_vec();
    let offset = flip_varint_byte(&mut out, field.value_start, field.value_end, rng)
        .ok_or(OperatorError::NoApplicableField)?;

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("path_leaf_field", field.field_number.to_string()),
            ("byte_offset", offset.to_string()),
        ],
        parse_preserving: "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_varint_field};
    use super::*;

    fn fixture_with_initializer_dtype() -> Vec<u8> {
        let mut tensor = Vec::new();
        tensor.extend(encode_varint_field(1, 4));
        tensor.extend(encode_varint_field(2, 1));
        let graph = encode_length_delimited(5, &tensor);
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn mutates_initializer_data_type() {
        let bytes = fixture_with_initializer_dtype();
        let mut rng = DeterministicRng::new(42);
        let result = apply(&bytes, &mut rng).expect("should find data_type");
        assert_eq!(result.parse_preserving, "yes");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
        let after = find_fields(&result.bytes, &[(7, 2), (5, 2), (2, 0)]);
        assert!(!after.is_empty());
    }

    #[test]
    fn errors_when_no_dtype_field() {
        let bytes = encode_length_delimited(7, &[]);
        let mut rng = DeterministicRng::new(1);
        assert!(matches!(
            apply(&bytes, &mut rng),
            Err(OperatorError::NoApplicableField)
        ));
    }
}
