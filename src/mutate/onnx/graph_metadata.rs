use super::{
    find_fields, flip_value_byte, flip_varint_byte, DeterministicRng, FieldRef, MutationOutput,
    OperatorError,
};

pub(crate) const NAME: &str = "graph_metadata";

enum Candidate {
    Varint(FieldRef),
    LengthDelimited(FieldRef),
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for f in find_fields(bytes, &[(1, 0)]) {
        candidates.push(Candidate::Varint(f));
    }
    for f in find_fields(bytes, &[(5, 0)]) {
        candidates.push(Candidate::Varint(f));
    }
    for &path in &[
        &[(2u32, 2u8)][..],
        &[(3, 2)][..],
        &[(4, 2)][..],
        &[(6, 2)][..],
        &[(7, 2), (2, 2)][..],
        &[(7, 2), (10, 2)][..],
        &[(14, 2), (1, 2)][..],
        &[(14, 2), (2, 2)][..],
    ] {
        for f in find_fields(bytes, path) {
            if f.value_end > f.value_start {
                candidates.push(Candidate::LengthDelimited(f));
            }
        }
    }

    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }

    let pick = rng.index(candidates.len());
    let mut out = bytes.to_vec();
    let (leaf_field, offset) = match &candidates[pick] {
        Candidate::Varint(f) => {
            let off = flip_varint_byte(&mut out, f.value_start, f.value_end, rng)
                .ok_or(OperatorError::NoApplicableField)?;
            (f.field_number, off)
        }
        Candidate::LengthDelimited(f) => {
            let off = flip_value_byte(&mut out, f.value_start, f.value_end, rng)
                .ok_or(OperatorError::NoApplicableField)?;
            (f.field_number, off)
        }
    };

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("path_leaf_field", leaf_field.to_string()),
            ("byte_offset", offset.to_string()),
        ],
        parse_preserving: "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{
        encode_length_delimited, encode_string_field, encode_varint_field,
    };
    use super::*;

    fn fixture_with_model_metadata() -> Vec<u8> {
        let mut model = Vec::new();
        model.extend(encode_varint_field(1, 8));
        model.extend(encode_string_field(2, "tool"));
        model.extend(encode_string_field(3, "0.1.0"));
        let graph = encode_string_field(2, "main");
        model.extend(encode_length_delimited(7, &graph));
        model
    }

    #[test]
    fn mutates_model_metadata() {
        let bytes = fixture_with_model_metadata();
        let mut rng = DeterministicRng::new(42);
        let result = apply(&bytes, &mut rng).expect("should find metadata");
        assert_eq!(result.parse_preserving, "yes");
        assert_ne!(result.bytes, bytes);
        assert_eq!(result.bytes.len(), bytes.len());
    }

    #[test]
    fn errors_on_empty_bytes() {
        let bytes: Vec<u8> = Vec::new();
        let mut rng = DeterministicRng::new(1);
        assert!(matches!(
            apply(&bytes, &mut rng),
            Err(OperatorError::NoApplicableField)
        ));
    }
}
