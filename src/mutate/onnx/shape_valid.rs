use super::{
    find_fields, pick_field, read_varint_value, write_varint_same_width, DeterministicRng,
    MutationOutput, OperatorError,
};

pub(crate) const NAME: &str = "shape_valid";

const SAFE_DIMS: &[u64] = &[1, 2, 3, 4, 8, 16];

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut candidates = Vec::new();
    candidates.extend(find_fields(bytes, &[(7, 2), (5, 2), (1, 0)]));
    candidates.extend(find_fields(
        bytes,
        &[(7, 2), (13, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)],
    ));
    candidates.extend(find_fields(
        bytes,
        &[(7, 2), (11, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)],
    ));
    candidates.extend(find_fields(
        bytes,
        &[(7, 2), (12, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)],
    ));

    let field = pick_field(&candidates, rng)
        .copied()
        .ok_or(OperatorError::NoApplicableField)?;
    let original = read_varint_value(bytes, field.value_start, field.value_end)
        .ok_or(OperatorError::NoApplicableField)?;
    let mut new_dim = SAFE_DIMS[rng.index(SAFE_DIMS.len())];
    if new_dim == original {
        new_dim = SAFE_DIMS
            .iter()
            .copied()
            .find(|value| *value != original)
            .ok_or(OperatorError::NoApplicableField)?;
    }

    let mut out = bytes.to_vec();
    write_varint_same_width(&mut out, field.value_start, field.value_end, new_dim)
        .ok_or(OperatorError::NoApplicableField)?;
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("path_leaf_field", field.field_number.to_string()),
            ("old_dim", original.to_string()),
            ("new_dim", new_dim.to_string()),
            ("value_offset", field.value_start.to_string()),
            ("mutation_level", "2".to_string()),
        ],
        parse_preserving: "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_varint_field};
    use super::*;

    fn fixture_with_shape_dim() -> Vec<u8> {
        let mut tensor = Vec::new();
        tensor.extend(encode_varint_field(1, 64));
        let graph = encode_length_delimited(5, &tensor);
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn writes_safe_dimension_without_changing_size() {
        let bytes = fixture_with_shape_dim();
        let mut rng = DeterministicRng::new(7);
        let result = apply(&bytes, &mut rng).expect("shape dim exists");
        assert_eq!(result.parse_preserving, "yes");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        let after = find_fields(&result.bytes, &[(7, 2), (5, 2), (1, 0)]);
        let value = read_varint_value(&result.bytes, after[0].value_start, after[0].value_end)
            .expect("dim remains readable");
        assert!(SAFE_DIMS.contains(&value));
    }
}
