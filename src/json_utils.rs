pub(crate) fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn extract_json_u64_field(json: &str, key: &str) -> Option<u64> {
    let key_pattern = format!("\"{}\":", key);
    let start = json.find(&key_pattern)? + key_pattern.len();
    let rest = &json[start..];
    let mut digits = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if !digits.is_empty() {
            break;
        }
        if !ch.is_ascii_whitespace() {
            return None;
        }
    }
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

pub(crate) fn extract_json_number_literal(json: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{}\":", key);
    let start = json.find(&key_pattern)? + key_pattern.len();
    let rest = &json[start..];

    let mut out = String::new();
    let mut started = false;
    for ch in rest.chars() {
        if !started {
            if ch.is_ascii_whitespace() {
                continue;
            }
            if ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.' {
                started = true;
                out.push(ch);
                continue;
            }
            return None;
        }

        if ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' || ch == '-' || ch == '+' {
            out.push(ch);
        } else {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub(crate) fn extract_json_string_literal(json: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{}\":", key);
    let start = json.find(&key_pattern)? + key_pattern.len();
    let rest = &json[start..];

    let mut value_start = None;
    for (idx, ch) in rest.char_indices() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch == '"' {
            value_start = Some(idx);
        }
        break;
    }
    let value_start = value_start?;
    let (value, _) = parse_json_string_literal_at(rest, value_start)?;
    Some(value)
}

pub(crate) fn extract_first_signature_top3_list(json: &str) -> Vec<String> {
    let key_pattern = "\"signature_top3\":";
    let start = match json.find(key_pattern) {
        Some(idx) => idx + key_pattern.len(),
        None => return Vec::new(),
    };
    let rest = &json[start..];
    let array_start = match rest.find('[') {
        Some(idx) => idx,
        None => return Vec::new(),
    };
    let array = &rest[array_start + 1..];

    let mut items = Vec::new();
    let mut scan = 0usize;
    while scan < array.len() && items.len() < 3 {
        let slice = &array[scan..];
        let mut value_start = None;
        for (idx, ch) in slice.char_indices() {
            if ch.is_ascii_whitespace() || ch == ',' {
                continue;
            }
            if ch == '"' {
                value_start = Some(idx);
            }
            break;
        }
        let Some(value_start) = value_start else {
            break;
        };
        let (value, consumed) = match parse_json_string_literal_at(slice, value_start) {
            Some(v) => v,
            None => break,
        };
        items.push(value);
        scan += consumed;
    }
    items
}

pub(crate) fn extract_first_signature_top1(json: &str) -> Option<String> {
    extract_first_signature_top3_list(json).into_iter().next()
}

fn parse_json_string_literal_at(input: &str, quote_index: usize) -> Option<(String, usize)> {
    let mut chars = input[quote_index..].char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }

    let mut out = String::new();
    let mut escaped = false;
    for (offset, ch) in chars {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => out.push(ch),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some((out, quote_index + offset + ch.len_utf8())),
            _ => out.push(ch),
        }
    }
    None
}
