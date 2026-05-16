use super::{
    find_fields, flip_value_byte, flip_varint_byte, DeterministicRng, FieldRef, MutationOutput,
    OperatorError,
};

pub(crate) const NAME: &str = "attribute";

enum Candidate {
    Varint(FieldRef),
    Fixed32(FieldRef),
    LengthDelimited(FieldRef),
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for f in find_fields(bytes, &[(7, 2), (1, 2), (5, 2), (4, 0)]) {
        candidates.push(Candidate::Varint(f));
    }
    for f in find_fields(bytes, &[(7, 2), (1, 2), (5, 2), (5, 5)]) {
        candidates.push(Candidate::Fixed32(f));
    }
    for f in find_fields(bytes, &[(7, 2), (1, 2), (5, 2), (6, 2)]) {
        if f.value_end > f.value_start {
            candidates.push(Candidate::LengthDelimited(f));
        }
    }

    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }

    let pick = rng.index(candidates.len());
    let mut out = bytes.to_vec();
    let (leaf_field, offset, kind) = match &candidates[pick] {
        Candidate::Varint(f) => {
            let off = flip_varint_byte(&mut out, f.value_start, f.value_end, rng)
                .ok_or(OperatorError::NoApplicableField)?;
            (f.field_number, off, "i")
        }
        Candidate::Fixed32(f) => {
            let off = flip_value_byte(&mut out, f.value_start, f.value_end, rng)
                .ok_or(OperatorError::NoApplicableField)?;
            (f.field_number, off, "f")
        }
        Candidate::LengthDelimited(f) => {
            let off = flip_value_byte(&mut out, f.value_start, f.value_end, rng)
                .ok_or(OperatorError::NoApplicableField)?;
            (f.field_number, off, "s")
        }
    };

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("path_leaf_field", leaf_field.to_string()),
            ("attr_kind", kind.to_string()),
            ("byte_offset", offset.to_string()),
        ],
        parse_preserving: "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_varint_field};
    use super::*;

    fn fixture_with_attribute() -> Vec<u8> {
        let mut attr = Vec::new();
        attr.extend(encode_varint_field(4, 123));
        let node = encode_length_delimited(5, &attr);
        let graph = encode_length_delimited(1, &node);
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn mutates_attribute_int() {
        let bytes = fixture_with_attribute();
        let mut rng = DeterministicRng::new(42);
        let result = apply(&bytes, &mut rng).expect("should find attribute");
        assert_eq!(result.parse_preserving, "yes");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
        let after = find_fields(&result.bytes, &[(7, 2), (1, 2), (5, 2), (4, 0)]);
        assert!(!after.is_empty());
    }

    #[test]
    fn errors_when_no_attribute_field() {
        let bytes = encode_length_delimited(7, &[]);
        let mut rng = DeterministicRng::new(1);
        assert!(matches!(
            apply(&bytes, &mut rng),
            Err(OperatorError::NoApplicableField)
        ));
    }
}
