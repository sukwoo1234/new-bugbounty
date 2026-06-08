use std::collections::BTreeMap;

use super::{
    find_fields, read_varint_value, write_varint_same_width, DeterministicRng, FieldRef,
    MutationOutput, OperatorError,
};

pub(crate) const NAME: &str = "dtype_wide";

const LEGAL_DTYPES: &[u64] = &[1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 16];

struct Candidate {
    scope: &'static str,
    original: u64,
    fields: Vec<FieldRef>,
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let mut candidates = Vec::new();
    extend_candidates(
        &mut candidates,
        bytes,
        "graph_annotations",
        &[
            &[(7u32, 2u8), (13, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (11, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (12, 2), (2, 2), (1, 2), (1, 0)][..],
        ],
    );
    extend_candidates(
        &mut candidates,
        bytes,
        "initializers",
        &[&[(7u32, 2u8), (5, 2), (2, 0)][..]],
    );
    extend_all_same_dtype_candidate(&mut candidates, bytes);

    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }

    let candidate = &candidates[rng.index(candidates.len())];
    let choices: Vec<u64> = LEGAL_DTYPES
        .iter()
        .copied()
        .filter(|dtype| *dtype != candidate.original)
        .collect();
    if choices.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let new_dtype = choices[rng.index(choices.len())];

    let mut out = bytes.to_vec();
    for field in &candidate.fields {
        write_varint_same_width(&mut out, field.value_start, field.value_end, new_dtype)
            .ok_or(OperatorError::NoApplicableField)?;
    }
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("scope", candidate.scope.to_string()),
            ("occurrences", candidate.fields.len().to_string()),
            ("old_dtype", candidate.original.to_string()),
            ("new_dtype", new_dtype.to_string()),
            ("mutation_level", "2".to_string()),
        ],
        parse_preserving: "yes",
    })
}

fn extend_candidates(
    candidates: &mut Vec<Candidate>,
    bytes: &[u8],
    scope: &'static str,
    paths: &[&[(u32, u8)]],
) {
    let mut by_dtype: BTreeMap<u64, Vec<FieldRef>> = BTreeMap::new();
    for path in paths {
        for field in find_fields(bytes, path) {
            let Some(dtype) = read_varint_value(bytes, field.value_start, field.value_end) else {
                continue;
            };
            if LEGAL_DTYPES.contains(&dtype) {
                by_dtype.entry(dtype).or_default().push(field);
            }
        }
    }
    for (original, fields) in by_dtype {
        if !fields.is_empty() {
            candidates.push(Candidate {
                scope,
                original,
                fields,
            });
        }
    }
}

fn extend_all_same_dtype_candidate(candidates: &mut Vec<Candidate>, bytes: &[u8]) {
    let mut by_dtype: BTreeMap<u64, Vec<FieldRef>> = BTreeMap::new();
    for path in [
        &[(7u32, 2u8), (13, 2), (2, 2), (1, 2), (1, 0)][..],
        &[(7, 2), (11, 2), (2, 2), (1, 2), (1, 0)][..],
        &[(7, 2), (12, 2), (2, 2), (1, 2), (1, 0)][..],
        &[(7, 2), (5, 2), (2, 0)][..],
    ] {
        for field in find_fields(bytes, path) {
            let Some(dtype) = read_varint_value(bytes, field.value_start, field.value_end) else {
                continue;
            };
            if LEGAL_DTYPES.contains(&dtype) {
                by_dtype.entry(dtype).or_default().push(field);
            }
        }
    }
    for (original, fields) in by_dtype {
        if fields.len() >= 2 {
            candidates.push(Candidate {
                scope: "all_same_dtype",
                original,
                fields,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_varint_field};
    use super::*;

    fn fixture_with_annotation_and_initializer_dtype() -> Vec<u8> {
        let mut tensor_type = Vec::new();
        tensor_type.extend(encode_varint_field(1, 1));
        let type_proto = encode_length_delimited(1, &tensor_type);
        let value_info = encode_length_delimited(2, &type_proto);

        let mut initializer = Vec::new();
        initializer.extend(encode_varint_field(2, 1));

        let mut graph = Vec::new();
        graph.extend(encode_length_delimited(11, &value_info));
        graph.extend(encode_length_delimited(5, &initializer));
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn writes_legal_dtype_without_changing_size() {
        let bytes = fixture_with_annotation_and_initializer_dtype();
        let mut rng = DeterministicRng::new(3);
        let result = apply(&bytes, &mut rng).expect("dtype candidate exists");
        assert_eq!(result.parse_preserving, "yes");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        assert_eq!(
            result
                .operator_params
                .iter()
                .find(|(key, _)| *key == "mutation_level")
                .map(|(_, value)| value.as_str()),
            Some("2")
        );
    }

    #[test]
    fn errors_when_no_legal_dtype_exists() {
        let bytes = encode_length_delimited(7, &[]);
        let mut rng = DeterministicRng::new(1);
        assert!(matches!(
            apply(&bytes, &mut rng),
            Err(OperatorError::NoApplicableField)
        ));
    }
}
