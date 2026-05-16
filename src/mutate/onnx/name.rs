use super::{
    find_fields, flip_value_byte, pick_field, DeterministicRng, MutationOutput, OperatorError,
};

pub(crate) const NAME: &str = "name";

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut candidates = Vec::new();
    candidates.extend(find_fields(bytes, &[(7, 2), (1, 2), (3, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (1, 2), (1, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (1, 2), (2, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (1, 2), (4, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (5, 2), (8, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (13, 2), (1, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (11, 2), (1, 2)]));
    candidates.extend(find_fields(bytes, &[(7, 2), (12, 2), (1, 2)]));

    candidates.retain(|f| f.value_end > f.value_start);

    let field = pick_field(&candidates, rng)
        .copied()
        .ok_or(OperatorError::NoApplicableField)?;
    let mut out = bytes.to_vec();
    let offset = flip_value_byte(&mut out, field.value_start, field.value_end, rng)
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
    use super::super::test_fixtures::{encode_length_delimited, encode_string_field};
    use super::*;

    fn fixture_with_node_name() -> Vec<u8> {
        let mut node = Vec::new();
        node.extend(encode_string_field(3, "Conv_0"));
        node.extend(encode_string_field(4, "Conv"));
        let graph = encode_length_delimited(1, &node);
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn mutates_node_name() {
        let bytes = fixture_with_node_name();
        let mut rng = DeterministicRng::new(42);
        let result = apply(&bytes, &mut rng).expect("should find name");
        assert_eq!(result.parse_preserving, "yes");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
        let after = find_fields(&result.bytes, &[(7, 2), (1, 2), (3, 2)]);
        assert!(!after.is_empty());
    }

    #[test]
    fn errors_when_no_name_field() {
        let bytes = encode_length_delimited(7, &[]);
        let mut rng = DeterministicRng::new(1);
        assert!(matches!(
            apply(&bytes, &mut rng),
            Err(OperatorError::NoApplicableField)
        ));
    }
}
