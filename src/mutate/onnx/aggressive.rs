use super::{
    find_fields, read_varint_value, write_varint_same_width, DeterministicRng, FieldRef,
    MutationOutput, OperatorError,
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
    99999,
    1_048_576,
    i32::MAX as u64,
    u32::MAX as u64,
    u64::MAX,
];

const TARGET_ATTRIBUTES: &[&str] = &[
    "kernel_shape",
    "strides",
    "dilations",
    "pads",
    "group",
    "axis",
    "axes",
    "shape",
];

const OP_TYPE_REPLACEMENTS: &[&str] = &[
    "Abs",
    "Add",
    "And",
    "Div",
    "Erf",
    "Exp",
    "Log",
    "Max",
    "Min",
    "Mul",
    "Neg",
    "Not",
    "Pow",
    "Relu",
    "Sign",
    "Sqrt",
    "Sub",
    "Tanh",
    "Conv",
    "Gemm",
    "Cast",
    "Clip",
    "Tile",
    "Where",
    "Equal",
    "Shape",
    "Slice",
    "Split",
    "Concat",
    "Expand",
    "Gather",
    "MatMul",
    "NonMax",
    "Reduce",
    "Resize",
    "ArgMax",
    "ArgMin",
    "Flatten",
    "MaxPool",
    "Squeeze",
    "Reshape",
    "Unsqueeze",
    "Transpose",
    "AveragePool",
];

const RAW_PATTERNS: &[&[u8]] = &[
    &[0x00, 0x00, 0x00, 0x00],
    &[0xff, 0xff, 0xff, 0xff],
    &[0x00, 0x00, 0x80, 0x7f],
    &[0x00, 0x00, 0x80, 0xff],
    &[0xff, 0xff, 0x7f, 0x7f],
    &[0x01, 0x00, 0x00, 0x00],
];

#[derive(Clone)]
enum Candidate {
    BoundaryVarint {
        scope: &'static str,
        attribute_name: Option<String>,
        field: FieldRef,
    },
    ResizableAttributeVarint {
        scope: &'static str,
        attribute_name: String,
        chain: AttributeChain,
        leaf: ProtoField,
        original: u64,
    },
    OpTypeSwap {
        field: FieldRef,
        old_op: String,
        new_ops: Vec<&'static str>,
    },
    RawDataPattern {
        field: FieldRef,
    },
}

#[derive(Clone, Copy)]
struct ProtoField {
    field_number: u32,
    wire_type: u8,
    tag_start: usize,
    value_start: usize,
    value_end: usize,
}

#[derive(Clone, Copy)]
struct AttributeChain {
    graph: ProtoField,
    node: ProtoField,
    attribute: ProtoField,
    packed: Option<ProtoField>,
}

pub(crate) fn apply(
    bytes: &[u8],
    rng: &mut DeterministicRng,
) -> Result<MutationOutput, OperatorError> {
    let candidates = collect_candidates(bytes);
    if candidates.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    match &candidates[rng.index(candidates.len())] {
        Candidate::BoundaryVarint {
            scope,
            attribute_name,
            field,
        } => apply_boundary_varint(bytes, rng, scope, attribute_name.as_deref(), *field),
        Candidate::ResizableAttributeVarint {
            scope,
            attribute_name,
            chain,
            leaf,
            original,
        } => apply_resizable_attribute_varint(
            bytes,
            rng,
            scope,
            attribute_name,
            *chain,
            *leaf,
            *original,
        ),
        Candidate::OpTypeSwap {
            field,
            old_op,
            new_ops,
        } => apply_op_type_swap(bytes, rng, *field, old_op, new_ops),
        Candidate::RawDataPattern { field } => apply_raw_data_pattern(bytes, rng, *field),
    }
}

fn collect_candidates(bytes: &[u8]) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    extend_boundary_candidates(
        &mut candidates,
        bytes,
        "initializer_dim",
        &[&[(7u32, 2u8), (5, 2), (1, 0)][..]],
    );
    extend_boundary_candidates(
        &mut candidates,
        bytes,
        "value_info_dim",
        &[
            &[(7u32, 2u8), (13, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (11, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)][..],
            &[(7, 2), (12, 2), (2, 2), (1, 2), (2, 2), (1, 2), (1, 0)][..],
        ],
    );
    extend_named_attribute_candidates(&mut candidates, bytes);
    extend_resizable_named_attribute_candidates(&mut candidates, bytes);
    extend_op_type_candidates(&mut candidates, bytes);
    extend_raw_data_candidates(&mut candidates, bytes);
    candidates
        .into_iter()
        .filter(|candidate| candidate_is_viable(bytes, candidate))
        .collect()
}

fn extend_boundary_candidates(
    candidates: &mut Vec<Candidate>,
    bytes: &[u8],
    scope: &'static str,
    paths: &[&[(u32, u8)]],
) {
    for path in paths {
        for field in find_fields(bytes, path) {
            candidates.push(Candidate::BoundaryVarint {
                scope,
                attribute_name: None,
                field,
            });
        }
    }
}

fn extend_named_attribute_candidates(candidates: &mut Vec<Candidate>, bytes: &[u8]) {
    for attribute in find_fields(bytes, &[(7, 2), (1, 2), (5, 2)]) {
        let fields = scan_proto_range(bytes, attribute.value_start, attribute.value_end);
        let attribute_name = fields
            .iter()
            .find(|field| field.field_number == 1 && field.wire_type == 2)
            .and_then(|field| field_text(bytes, *field));
        let is_target = attribute_name
            .as_deref()
            .map(|name| TARGET_ATTRIBUTES.contains(&name))
            .unwrap_or(false);

        for field in &fields {
            if field.field_number == 3 && field.wire_type == 0 {
                candidates.push(Candidate::BoundaryVarint {
                    scope: if is_target {
                        "named_attribute_i"
                    } else {
                        "attribute_i"
                    },
                    attribute_name: attribute_name.clone(),
                    field: *field,
                });
            }
            if field.field_number == 8 && field.wire_type == 0 {
                candidates.push(Candidate::BoundaryVarint {
                    scope: if is_target {
                        "named_attribute_ints"
                    } else {
                        "attribute_ints"
                    },
                    attribute_name: attribute_name.clone(),
                    field: *field,
                });
            }
            if field.field_number == 8 && field.wire_type == 2 {
                for packed in packed_varint_fields(bytes, *field) {
                    candidates.push(Candidate::BoundaryVarint {
                        scope: if is_target {
                            "named_attribute_ints_packed"
                        } else {
                            "attribute_ints_packed"
                        },
                        attribute_name: attribute_name.clone(),
                        field: packed,
                    });
                }
            }
        }
    }
}

fn extend_resizable_named_attribute_candidates(candidates: &mut Vec<Candidate>, bytes: &[u8]) {
    for graph in scan_proto_spans(bytes, 0, bytes.len()) {
        if graph.field_number != 7 || graph.wire_type != 2 {
            continue;
        }
        for node in scan_proto_spans(bytes, graph.value_start, graph.value_end) {
            if node.field_number != 1 || node.wire_type != 2 {
                continue;
            }
            for attribute in scan_proto_spans(bytes, node.value_start, node.value_end) {
                if attribute.field_number != 5 || attribute.wire_type != 2 {
                    continue;
                }
                extend_resizable_attribute_fields(candidates, bytes, graph, node, attribute);
            }
        }
    }
}

fn extend_resizable_attribute_fields(
    candidates: &mut Vec<Candidate>,
    bytes: &[u8],
    graph: ProtoField,
    node: ProtoField,
    attribute: ProtoField,
) {
    let fields = scan_proto_spans(bytes, attribute.value_start, attribute.value_end);
    let Some(attribute_name) = fields
        .iter()
        .find(|field| field.field_number == 1 && field.wire_type == 2)
        .and_then(|field| proto_field_text(bytes, *field))
    else {
        return;
    };
    if !TARGET_ATTRIBUTES.contains(&attribute_name.as_str()) {
        return;
    }

    for field in &fields {
        if field.field_number == 3 && field.wire_type == 0 {
            if let Some(original) = read_varint_value(bytes, field.value_start, field.value_end) {
                candidates.push(Candidate::ResizableAttributeVarint {
                    scope: "semantic_attribute_i",
                    attribute_name: attribute_name.clone(),
                    chain: AttributeChain {
                        graph,
                        node,
                        attribute,
                        packed: None,
                    },
                    leaf: *field,
                    original,
                });
            }
        }
        if field.field_number == 8 && field.wire_type == 0 {
            if let Some(original) = read_varint_value(bytes, field.value_start, field.value_end) {
                candidates.push(Candidate::ResizableAttributeVarint {
                    scope: "semantic_attribute_ints",
                    attribute_name: attribute_name.clone(),
                    chain: AttributeChain {
                        graph,
                        node,
                        attribute,
                        packed: None,
                    },
                    leaf: *field,
                    original,
                });
            }
        }
        if field.field_number == 8 && field.wire_type == 2 {
            for leaf in packed_varint_proto_fields(bytes, *field) {
                if let Some(original) = read_varint_value(bytes, leaf.value_start, leaf.value_end) {
                    candidates.push(Candidate::ResizableAttributeVarint {
                        scope: "semantic_attribute_ints_packed",
                        attribute_name: attribute_name.clone(),
                        chain: AttributeChain {
                            graph,
                            node,
                            attribute,
                            packed: Some(*field),
                        },
                        leaf,
                        original,
                    });
                }
            }
        }
    }
}

fn extend_op_type_candidates(candidates: &mut Vec<Candidate>, bytes: &[u8]) {
    for field in find_fields(bytes, &[(7, 2), (1, 2), (4, 2)]) {
        let Some(old_op) = field_text(bytes, field) else {
            continue;
        };
        let new_ops: Vec<&'static str> = OP_TYPE_REPLACEMENTS
            .iter()
            .copied()
            .filter(|op| op.len() == old_op.len() && *op != old_op)
            .collect();
        if !new_ops.is_empty() {
            candidates.push(Candidate::OpTypeSwap {
                field,
                old_op,
                new_ops,
            });
        }
    }
}

fn extend_raw_data_candidates(candidates: &mut Vec<Candidate>, bytes: &[u8]) {
    for field in find_fields(bytes, &[(7, 2), (5, 2), (9, 2)]) {
        if field.value_end.saturating_sub(field.value_start) >= 4 {
            candidates.push(Candidate::RawDataPattern { field });
        }
    }
}

fn candidate_is_viable(bytes: &[u8], candidate: &Candidate) -> bool {
    match candidate {
        Candidate::BoundaryVarint { field, .. } => {
            read_varint_value(bytes, field.value_start, field.value_end)
                .map(|original| !viable_boundary_values(*field, original).is_empty())
                .unwrap_or(false)
        }
        Candidate::ResizableAttributeVarint { original, .. } => {
            BOUNDARY_VALUES.iter().any(|value| value != original)
        }
        Candidate::OpTypeSwap { new_ops, .. } => !new_ops.is_empty(),
        Candidate::RawDataPattern { field } => {
            field.value_end.saturating_sub(field.value_start) >= 4
        }
    }
}

fn apply_resizable_attribute_varint(
    bytes: &[u8],
    rng: &mut DeterministicRng,
    scope: &'static str,
    attribute_name: &str,
    chain: AttributeChain,
    leaf: ProtoField,
    original: u64,
) -> Result<MutationOutput, OperatorError> {
    let choices: Vec<u64> = BOUNDARY_VALUES
        .iter()
        .copied()
        .filter(|value| *value != original)
        .collect();
    if choices.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let new_value = choices[rng.index(choices.len())];
    let encoded_value = encode_varint(new_value);

    let mut new_attribute_payload =
        bytes[chain.attribute.value_start..chain.attribute.value_end].to_vec();
    let semantic_resize_kind;
    if let Some(packed) = chain.packed {
        let mut new_packed_payload = bytes[packed.value_start..packed.value_end].to_vec();
        replace_relative_range(
            &mut new_packed_payload,
            leaf.value_start - packed.value_start,
            leaf.value_end - packed.value_start,
            &encoded_value,
        )?;
        let new_packed_field =
            encode_length_delimited_field(packed.field_number, &new_packed_payload);
        replace_relative_range(
            &mut new_attribute_payload,
            packed.tag_start - chain.attribute.value_start,
            packed.value_end - chain.attribute.value_start,
            &new_packed_field,
        )?;
        semantic_resize_kind = "packed";
    } else {
        replace_relative_range(
            &mut new_attribute_payload,
            leaf.value_start - chain.attribute.value_start,
            leaf.value_end - chain.attribute.value_start,
            &encoded_value,
        )?;
        semantic_resize_kind = "direct";
    }

    let new_attribute_field =
        encode_length_delimited_field(chain.attribute.field_number, &new_attribute_payload);
    let mut new_node_payload = bytes[chain.node.value_start..chain.node.value_end].to_vec();
    replace_relative_range(
        &mut new_node_payload,
        chain.attribute.tag_start - chain.node.value_start,
        chain.attribute.value_end - chain.node.value_start,
        &new_attribute_field,
    )?;

    let new_node_field = encode_length_delimited_field(chain.node.field_number, &new_node_payload);
    let mut new_graph_payload = bytes[chain.graph.value_start..chain.graph.value_end].to_vec();
    replace_relative_range(
        &mut new_graph_payload,
        chain.node.tag_start - chain.graph.value_start,
        chain.node.value_end - chain.graph.value_start,
        &new_node_field,
    )?;

    let new_graph_field =
        encode_length_delimited_field(chain.graph.field_number, &new_graph_payload);
    let mut out = bytes.to_vec();
    replace_relative_range(
        &mut out,
        chain.graph.tag_start,
        chain.graph.value_end,
        &new_graph_field,
    )?;
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("scope", scope.to_string()),
            ("attribute_name", attribute_name.to_string()),
            ("path_leaf_field", leaf.field_number.to_string()),
            ("old_value", original.to_string()),
            ("new_value", new_value.to_string()),
            ("boundary", boundary_label(new_value).to_string()),
            ("semantic_resize", "yes".to_string()),
            ("semantic_resize_kind", semantic_resize_kind.to_string()),
            ("old_size", bytes.len().to_string()),
            ("mutation_level", "3".to_string()),
        ],
        parse_preserving: "yes",
    })
}

fn apply_boundary_varint(
    bytes: &[u8],
    rng: &mut DeterministicRng,
    scope: &'static str,
    attribute_name: Option<&str>,
    field: FieldRef,
) -> Result<MutationOutput, OperatorError> {
    let original = read_varint_value(bytes, field.value_start, field.value_end)
        .ok_or(OperatorError::NoApplicableField)?;
    let choices = viable_boundary_values(field, original);
    if choices.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let new_value = choices[rng.index(choices.len())];

    let mut out = bytes.to_vec();
    write_varint_same_width(&mut out, field.value_start, field.value_end, new_value)
        .ok_or(OperatorError::NoApplicableField)?;
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }

    let mut operator_params = vec![
        ("scope", scope.to_string()),
        ("path_leaf_field", field.field_number.to_string()),
        ("old_value", original.to_string()),
        ("new_value", new_value.to_string()),
        ("boundary", boundary_label(new_value).to_string()),
        ("value_offset", field.value_start.to_string()),
        (
            "varint_width",
            (field.value_end - field.value_start).to_string(),
        ),
        ("mutation_level", "3".to_string()),
    ];
    if let Some(name) = attribute_name {
        operator_params.push(("attribute_name", name.to_string()));
    }

    Ok(MutationOutput {
        bytes: out,
        operator_params,
        parse_preserving: "yes",
    })
}

fn apply_op_type_swap(
    bytes: &[u8],
    rng: &mut DeterministicRng,
    field: FieldRef,
    old_op: &str,
    new_ops: &[&'static str],
) -> Result<MutationOutput, OperatorError> {
    let new_op = new_ops[rng.index(new_ops.len())];
    let mut out = bytes.to_vec();
    out[field.value_start..field.value_end].copy_from_slice(new_op.as_bytes());
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }
    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("scope", "op_type".to_string()),
            ("old_op", old_op.to_string()),
            ("new_op", new_op.to_string()),
            ("value_offset", field.value_start.to_string()),
            ("mutation_level", "3".to_string()),
        ],
        parse_preserving: "yes",
    })
}

fn apply_raw_data_pattern(
    bytes: &[u8],
    rng: &mut DeterministicRng,
    field: FieldRef,
) -> Result<MutationOutput, OperatorError> {
    let patterns: Vec<&[u8]> = RAW_PATTERNS
        .iter()
        .copied()
        .filter(|pattern| pattern.len() <= field.value_end.saturating_sub(field.value_start))
        .collect();
    if patterns.is_empty() {
        return Err(OperatorError::NoApplicableField);
    }
    let pattern = patterns[rng.index(patterns.len())];
    let max_start = field.value_end - field.value_start - pattern.len();
    let offset = field.value_start + rng.index(max_start + 1);
    let mut out = bytes.to_vec();
    out[offset..offset + pattern.len()].copy_from_slice(pattern);
    if out == bytes {
        return Err(OperatorError::NoApplicableField);
    }
    Ok(MutationOutput {
        bytes: out,
        operator_params: vec![
            ("scope", "tensor_raw_data".to_string()),
            ("pattern_len", pattern.len().to_string()),
            ("value_offset", offset.to_string()),
            ("mutation_level", "3".to_string()),
        ],
        parse_preserving: "yes",
    })
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
        99999 => "large_99999",
        1_048_576 => "large_2pow20",
        value if value == i32::MAX as u64 => "int32_max",
        value if value == u32::MAX as u64 => "uint32_max",
        u64::MAX => "int64_minus_one",
        _ => "boundary",
    }
}

fn field_text(bytes: &[u8], field: FieldRef) -> Option<String> {
    if field.value_end <= field.value_start || field.value_end > bytes.len() {
        return None;
    }
    let text = std::str::from_utf8(&bytes[field.value_start..field.value_end]).ok()?;
    if text
        .bytes()
        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        Some(text.to_string())
    } else {
        None
    }
}

fn packed_varint_fields(bytes: &[u8], field: FieldRef) -> Vec<FieldRef> {
    let mut fields = Vec::new();
    let mut cursor = field.value_start;
    let end = field.value_end.min(bytes.len());
    while cursor < end {
        let Some((_, next)) = read_varint_in(bytes, cursor, end) else {
            break;
        };
        fields.push(FieldRef {
            field_number: field.field_number,
            wire_type: 0,
            value_start: cursor,
            value_end: next,
        });
        cursor = next;
    }
    fields
}

fn packed_varint_proto_fields(bytes: &[u8], field: ProtoField) -> Vec<ProtoField> {
    let mut fields = Vec::new();
    let mut cursor = field.value_start;
    let end = field.value_end.min(bytes.len());
    while cursor < end {
        let Some((_, next)) = read_varint_in(bytes, cursor, end) else {
            break;
        };
        fields.push(ProtoField {
            field_number: field.field_number,
            wire_type: 0,
            tag_start: cursor,
            value_start: cursor,
            value_end: next,
        });
        cursor = next;
    }
    fields
}

fn scan_proto_spans(bytes: &[u8], start: usize, end: usize) -> Vec<ProtoField> {
    let mut fields = Vec::new();
    let mut cursor = start;
    let end = end.min(bytes.len());
    while cursor < end && fields.len() < 4096 {
        let tag_start = cursor;
        let Some((tag, after_tag)) = read_varint_in(bytes, cursor, end) else {
            break;
        };
        if tag == 0 {
            break;
        }
        let wire_type = (tag & 0x7) as u8;
        let field_number = (tag >> 3) as u32;
        if field_number == 0 {
            break;
        }
        let (value_start, value_end) = match wire_type {
            0 => {
                let Some((_, value_end)) = read_varint_in(bytes, after_tag, end) else {
                    break;
                };
                (after_tag, value_end)
            }
            1 => {
                let Some(value_end) = after_tag.checked_add(8) else {
                    break;
                };
                if value_end > end {
                    break;
                }
                (after_tag, value_end)
            }
            2 => {
                let Some((length, value_start)) = read_varint_in(bytes, after_tag, end) else {
                    break;
                };
                let Ok(length) = usize::try_from(length) else {
                    break;
                };
                let Some(value_end) = value_start.checked_add(length) else {
                    break;
                };
                if value_end > end {
                    break;
                }
                (value_start, value_end)
            }
            5 => {
                let Some(value_end) = after_tag.checked_add(4) else {
                    break;
                };
                if value_end > end {
                    break;
                }
                (after_tag, value_end)
            }
            _ => break,
        };
        fields.push(ProtoField {
            field_number,
            wire_type,
            tag_start,
            value_start,
            value_end,
        });
        cursor = value_end;
    }
    fields
}

fn scan_proto_range(bytes: &[u8], start: usize, end: usize) -> Vec<FieldRef> {
    let mut fields = Vec::new();
    let mut cursor = start;
    let end = end.min(bytes.len());
    while cursor < end && fields.len() < 512 {
        let Some((tag, after_tag)) = read_varint_in(bytes, cursor, end) else {
            break;
        };
        if tag == 0 {
            break;
        }
        let wire_type = (tag & 0x7) as u8;
        let field_number = (tag >> 3) as u32;
        if field_number == 0 {
            break;
        }
        let (value_start, value_end) = match wire_type {
            0 => {
                let Some((_, value_end)) = read_varint_in(bytes, after_tag, end) else {
                    break;
                };
                (after_tag, value_end)
            }
            1 => {
                let Some(value_end) = after_tag.checked_add(8) else {
                    break;
                };
                if value_end > end {
                    break;
                }
                (after_tag, value_end)
            }
            2 => {
                let Some((length, value_start)) = read_varint_in(bytes, after_tag, end) else {
                    break;
                };
                let Ok(length) = usize::try_from(length) else {
                    break;
                };
                let Some(value_end) = value_start.checked_add(length) else {
                    break;
                };
                if value_end > end {
                    break;
                }
                (value_start, value_end)
            }
            5 => {
                let Some(value_end) = after_tag.checked_add(4) else {
                    break;
                };
                if value_end > end {
                    break;
                }
                (after_tag, value_end)
            }
            _ => break,
        };
        fields.push(FieldRef {
            field_number,
            wire_type,
            value_start,
            value_end,
        });
        cursor = value_end;
    }
    fields
}

fn proto_field_text(bytes: &[u8], field: ProtoField) -> Option<String> {
    if field.value_end <= field.value_start || field.value_end > bytes.len() {
        return None;
    }
    let text = std::str::from_utf8(&bytes[field.value_start..field.value_end]).ok()?;
    if text
        .bytes()
        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        Some(text.to_string())
    } else {
        None
    }
}

fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
            out.push(byte);
        } else {
            out.push(byte);
            return out;
        }
    }
}

fn encode_length_delimited_field(field_number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = encode_varint(((field_number as u64) << 3) | 2);
    out.extend(encode_varint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn replace_relative_range(
    bytes: &mut Vec<u8>,
    start: usize,
    end: usize,
    replacement: &[u8],
) -> Result<(), OperatorError> {
    if start > end || end > bytes.len() {
        return Err(OperatorError::NoApplicableField);
    }
    bytes.splice(start..end, replacement.iter().copied());
    Ok(())
}

fn read_varint_in(bytes: &[u8], start: usize, end: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut cursor = start;
    let end = end.min(bytes.len());
    while cursor < end && shift < 64 {
        let byte = bytes[cursor];
        value |= ((byte & 0x7f) as u64) << shift;
        cursor += 1;
        if byte & 0x80 == 0 {
            return Some((value, cursor));
        }
        shift += 7;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{
        encode_length_delimited, encode_string_field, encode_varint_field,
    };
    use super::*;

    fn fixture_with_initializer_and_attr() -> Vec<u8> {
        let mut initializer = Vec::new();
        initializer.extend(encode_varint_field(1, 1));

        let mut attr = Vec::new();
        attr.extend(encode_string_field(1, "strides"));
        attr.extend(encode_varint_field(3, 1));
        attr.extend(encode_varint_field(8, 2));
        let node = encode_length_delimited(5, &attr);

        let mut graph = Vec::new();
        graph.extend(encode_length_delimited(5, &initializer));
        graph.extend(encode_length_delimited(1, &node));
        encode_length_delimited(7, &graph)
    }

    fn fixture_with_only_op_type() -> Vec<u8> {
        let mut node = Vec::new();
        node.extend(encode_string_field(4, "Conv"));
        let graph = encode_length_delimited(1, &node);
        encode_length_delimited(7, &graph)
    }

    fn fixture_with_raw_data() -> Vec<u8> {
        let mut initializer = Vec::new();
        initializer.extend(encode_length_delimited(9, &[1, 2, 3, 4, 5, 6, 7, 8]));
        let graph = encode_length_delimited(5, &initializer);
        encode_length_delimited(7, &graph)
    }

    fn fixture_with_packed_strides() -> Vec<u8> {
        let mut packed_ints = Vec::new();
        packed_ints.extend(encode_varint(1));
        packed_ints.extend(encode_varint(2));

        let mut attr = Vec::new();
        attr.extend(encode_string_field(1, "strides"));
        attr.extend(encode_length_delimited(8, &packed_ints));
        let node = encode_length_delimited(5, &attr);
        let graph = encode_length_delimited(1, &node);
        encode_length_delimited(7, &graph)
    }

    #[test]
    fn writes_boundary_without_changing_wire_size() {
        // This operator has candidates that resize on purpose (semantic_resize) and
        // candidates that write a boundary value into the SAME varint width. Only the
        // second kind is what this test is about, and picking it with one hard-coded
        // seed was picking it by luck: the seed selected a resizing candidate as soon
        // as the rng stream changed, and the test failed for a reason it does not name.
        let bytes = fixture_with_initializer_and_attr();
        let mut checked = 0usize;
        for seed in 0..256u64 {
            let mut rng = DeterministicRng::new(seed);
            let result = apply(&bytes, &mut rng).expect("aggressive candidate exists");
            let param = |key: &str| {
                result
                    .operator_params
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.as_str())
            };
            if param("semantic_resize").is_some() || param("varint_width").is_none() {
                continue;
            }
            assert_eq!(result.parse_preserving, "yes");
            assert_eq!(
                result.bytes.len(),
                bytes.len(),
                "seed {seed}: a same-width boundary write must not change the wire size"
            );
            assert_ne!(result.bytes, bytes);
            assert_eq!(param("mutation_level"), Some("3"));
            checked += 1;
        }
        assert!(
            checked > 0,
            "no seed produced a same-width boundary write, so this test proved nothing"
        );
    }

    #[test]
    fn named_attribute_candidates_record_name() {
        let bytes = fixture_with_initializer_and_attr();
        let mut saw_named_attribute = false;
        for seed in 0..256 {
            let mut rng = DeterministicRng::new(seed);
            let result = apply(&bytes, &mut rng).expect("aggressive candidate exists");
            saw_named_attribute |= result
                .operator_params
                .iter()
                .any(|(key, value)| *key == "attribute_name" && value == "strides");
        }
        assert!(
            saw_named_attribute,
            "named stride attribute must be targetable"
        );
    }

    #[test]
    fn can_generate_zero_for_one_byte_fields() {
        let bytes = fixture_with_initializer_and_attr();
        let mut saw_zero = false;
        for seed in 0..256 {
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
    fn semantic_resize_can_write_minus_one_and_grow_model() {
        let bytes = fixture_with_initializer_and_attr();
        let mut observed = None;
        for seed in 0..4096 {
            let mut rng = DeterministicRng::new(seed);
            let result = apply(&bytes, &mut rng).expect("aggressive candidate exists");
            let semantic_resize = result
                .operator_params
                .iter()
                .any(|(key, value)| *key == "semantic_resize" && value == "yes");
            let minus_one = result
                .operator_params
                .iter()
                .any(|(key, value)| *key == "new_value" && value == &u64::MAX.to_string());
            if semantic_resize && minus_one {
                observed = Some(result);
                break;
            }
        }
        let result = observed.expect("semantic resize must be able to write protobuf -1");
        assert!(result.bytes.len() > bytes.len());
        assert!(!find_fields(&result.bytes, &[(7, 2), (1, 2), (5, 2), (1, 2)]).is_empty());
    }

    #[test]
    fn semantic_resize_handles_packed_int_attributes() {
        let bytes = fixture_with_packed_strides();
        let mut observed = None;
        for seed in 0..4096 {
            let mut rng = DeterministicRng::new(seed);
            let result = apply(&bytes, &mut rng).expect("aggressive candidate exists");
            let semantic_packed = result
                .operator_params
                .iter()
                .any(|(key, value)| *key == "scope" && value == "semantic_attribute_ints_packed");
            let resized = result
                .operator_params
                .iter()
                .any(|(key, value)| *key == "semantic_resize" && value == "yes");
            if semantic_packed && resized {
                observed = Some(result);
                break;
            }
        }
        let result = observed.expect("packed repeated ints must be semantically resizable");
        assert_ne!(result.bytes, bytes);
        assert!(!find_fields(&result.bytes, &[(7, 2), (1, 2), (5, 2), (1, 2)]).is_empty());
    }

    #[test]
    fn swaps_op_type_with_same_wire_size() {
        let bytes = fixture_with_only_op_type();
        let mut rng = DeterministicRng::new(2);
        let result = apply(&bytes, &mut rng).expect("op type candidate exists");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        assert_eq!(
            result
                .operator_params
                .iter()
                .find(|(key, _)| *key == "scope")
                .map(|(_, value)| value.as_str()),
            Some("op_type")
        );
    }

    #[test]
    fn mutates_raw_tensor_data_with_boundary_pattern() {
        let bytes = fixture_with_raw_data();
        let mut rng = DeterministicRng::new(3);
        let result = apply(&bytes, &mut rng).expect("raw data candidate exists");
        assert_eq!(result.bytes.len(), bytes.len());
        assert_ne!(result.bytes, bytes);
        assert_eq!(
            result
                .operator_params
                .iter()
                .find(|(key, _)| *key == "scope")
                .map(|(_, value)| value.as_str()),
            Some("tensor_raw_data")
        );
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
