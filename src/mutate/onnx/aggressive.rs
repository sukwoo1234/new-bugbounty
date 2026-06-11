use super::{
    find_fields, pick_field, read_varint_value, write_varint_same_width, DeterministicRng,
    FieldRef, MutationOutput, OperatorError,
};

pub(crate) const NAME: &str = "aggressive";

const BOUNDARY_VALUES: &[u64] = &[
    0,
    1,
    2,
    3,
    7,
    16,
    127,
    255,
    1024,
    65535,
    1_048_576,
    i32::MAX as u64,
    u32::MAX as u64,
    u64::MAX,
];

#[derive(Clone, Copy)]
struct Candidate {
    scope: &'static str,
    field: FieldRef,
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let candidates = collect_candidates(bytes);
    let candidate = pick_field_like(&candidates, rng).ok_or(OperatorError::NoApplicableField)?;
    let original = read_varint_value(
        bytes,
        candidate.field.value_start,
        candidate.field.value_end,
    )
    .ok_or(OperatorError::NoApplicableField)?;
    let choices = viable_boundary_values(candidate.field, original);
    if choices.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let new_value = choices[rng.index(choices.len())];

    let mut out = bytes.to_vec();
    write_varint_same_width(
        &mut out,
        candidate.field.value_start,
        candidate.field.value_end,
        new_value,
    )
    .ok_or(OperatorError::NoApplicableField)?;
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("scope", candidate.scope.to_string()),
            ("path_leaf_field", candidate.field.field_number.to_string()),
            ("old_value", original.to_string()),
            ("new_value", new_value.to_string()),
            ("boundary", boundary_label(new_value).to_string()),
            ("value_offset", candidate.field.value_start.to_string()),
            (
                "varint_width",
                (candidate.field.value_end - candidate.field.value_start).to_string(),
            ),
            ("mutation_level", "3".to_string()),
        ],
        parse_preserving: "yes",
    })
}

fn collect_candidates(bytes: &[u8]) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    extend_candidates(
        &mut candidates,
        bytes,
        "initializer_dim",
        &[&[(7u32, 2u8), (5, 2), (1, 0)][..]],
    );
    extend_candidates(
        &mut candidates,
        bytes,
        "value_info_dim",
        &[
            &[(7u32, 2u8), (13, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (11, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (12, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)][..],
        ],
    );
    extend_candidates(
        &mut candidates,
        bytes,
        "attribute_i",
        &[&[(7u32, 2u8), (1, 2), (5, 2), (3, 0)][..]],
    );
    extend_candidates(
        &mut candidates,
        bytes,
        "attribute_ints",
        &[&[(7u32, 2u8), (1, 2), (5, 2), (8, 0)][..]],
    );
    candidates
        .into_iter()
        .filter(|candidate| {
            read_varint_value(
                bytes,
                candidate.field.value_start,
                candidate.field.value_end,
            )
            .map(|original| !viable_boundary_values(candidate.field, original).is_empty())
            .unwrap_or(false)
        })
        .collect()
}

fn extend_candidates(
    candidates: &mut Vec<Candidate>,
    bytes: &[u8],
    scope: &'static str,
    paths: &[&[(u32, u8)]],
) {
    for path in paths {
        for field in find_fields(bytes, path) {
            candidates.push(Candidate { scope, field });
        }
    }
}

fn pick_field_like<'a>(
    candidates: &'a [Candidate],
    rng: &mut DeterministicRng,
) -> Option<&'a Candidate> {
    if candidates.is_empty() {
        None
    } else {
        let refs: Vec<FieldRef> = candidates.iter().map(|candidate| candidate.field).collect();
        let picked = pick_field(&refs, rng)?;
        candidates
            .iter()
            .find(|candidate| candidate.field.value_start == picked.value_start)
    }
}

fn viable_boundary_values(field: FieldRef, original: u64) -> Vec<u64> {
    let width = field.value_end.saturating_sub(field.value_start);
    BOUNDARY_VALUES
        .iter()
        .copied()
        .filter(|value| *value != original)
        .filter(|value| varint_width(*value) <= width)
        .collect()
}

fn varint_width(mut value: u64) -> usize {
    let mut width = 1;
    while value >= 0x80 {
        value >>= 7;
        width += 1;
    }
    width
}

fn boundary_label(value: u64) -> &'static str {
    match value {
        0 => "zero",
        1 => "one",
        2 => "two",
        127 => "int7_max",
        255 => "uint8_max",
        65535 => "uint16_max",
        1_048_576 => "large_2pow20",
        value if value == i32::MAX as u64 => "int32_max",
        value if value == u32::MAX as u64 => "uint32_max",
        u64::MAX => "int64_minus_one",
        _ => "boundary",
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{encode_length_delimited, encode_varint_field};
    use super::*;

    fn fixture_with_initializer_and_attr() -> Vec<u8> {
        let mut initializer = Vec::new();
        initializer.extend(encode_varint_field(1, 1));

        let mut attr = Vec::new();
        attr.extend(encode_varint_field(3, 1));
        attr.extend(encode_varint_field(8, 2));
        let node = encode_length_delimited(5, &attr);

        let mut graph = Vec::new();
        graph.extend(encode_length_delimited(5, &initializer));
        graph.extend(encode_length_delimited(1, &node));
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn writes_boundary_without_changing_wire_size() {
        let bytes = fixture_with_initializer_and_attr();
        let mut rng = DeterministicRng::new(11);
        let result = apply(&bytes, &mut rng).expect("aggressive candidate exists");
        assert_eq!(result.parse_preserving, "yes");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        assert_eq!(
            result
                .operator_params
                .iter()
                .find(|(key, _)| *key == "mutation_level")
                .map(|(_, value)| value.as_str()),
            Some("3")
        );
    }

    #[test]
    fn can_generate_zero_for_one_byte_fields() {
        let bytes = fixture_with_initializer_and_attr();
        let mut saw_zero = false;
        for seed in 0..128 {
            let mut rng = DeterministicRng::new(seed);
            let result = apply(&bytes, &mut rng).expect("aggressive candidate exists");
            saw_zero |= result
                .operator_params
                .iter()
                .any(|(key, value)| *key == "new_value" && value == "0");
        }
        assert!(saw_zero, "zero boundary must be reachable");
    }

    #[test]
    fn errors_when_no_aggressive_candidate_exists() {
        let bytes = encode_length_delimited(7, &[]);
        let mut rng = DeterministicRng::new(1);
        assert!(matches!(
            apply(&bytes, &mut rng),
            Err(OperatorError::NoApplicableField)
        ));
    }
}
