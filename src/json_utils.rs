pub(crate) fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // A19: RFC 8259 requires every U+0000..U+001F to be escaped. The short
            // forms above stay so output that is already valid is byte-identical;
            // the rest would otherwise make jq, python and the browser refuse the
            // whole file. DEL (0x7f) is legal unescaped and is left alone.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        let is_unreserved =
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/');
        if is_unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
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
    while let Some((offset, ch)) = chars.next() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                // What json_escape now writes, and what jq and python's json.dump
                // write for any control byte or non-ASCII character. Without this a
                // \u001b read back out of summary.json becomes the text "u001b".
                'u' => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        let (_, digit) = chars.next()?;
                        hex.push(digit);
                    }
                    let value = u32::from_str_radix(&hex, 16).ok()?;
                    // Surrogate halves are never emitted by this crate; a lone one
                    // becomes U+FFFD rather than desynchronising the scan.
                    out.push(char::from_u32(value).unwrap_or('\u{fffd}'));
                }
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

#[cfg(test)]
mod tests {
    use super::{extract_json_string_literal, json_escape};

    // A19: RFC 8259 requires every U+0000..U+001F to be escaped. The tool's own
    // mutator can flip an ONNX node-name byte to a control character, onnxruntime
    // echoes that name into its error text, and triage keeps it - so an unescaped
    // ESC lands in summary.json and jq/python refuse to parse the file.
    #[test]
    fn control_characters_are_escaped_so_the_json_stays_parseable() {
        let escaped = json_escape("node\u{1b}[31m\u{0}name");
        assert_eq!(escaped, "node\\u001b[31m\\u0000name");
    }

    #[test]
    fn the_short_escape_forms_are_kept_so_existing_output_is_unchanged() {
        assert_eq!(json_escape("a\nb\rc\td\"e\\f"), "a\\nb\\rc\\td\\\"e\\\\f");
    }

    // The reader has to understand what the writer now emits, or a control byte
    // would round-trip through report.rs as the literal text "u001b".
    #[test]
    fn the_reader_decodes_the_escapes_the_writer_emits() {
        let body = format!("{{\"crash_summary\": \"{}\"}}", json_escape("x\u{1b}[31my"));
        assert_eq!(
            extract_json_string_literal(&body, "crash_summary").as_deref(),
            Some("x\u{1b}[31my")
        );
    }

    #[test]
    fn the_reader_decodes_unicode_escapes_written_by_jq_and_python() {
        // jq and python's json.dump write \uXXXX for control bytes and non-ASCII.
        let body = "{\"path\": \"\\ud55c\\uae00\\u0008\\u000c\"}";
        assert_eq!(
            extract_json_string_literal(body, "path").as_deref(),
            Some("\u{d55c}\u{ae00}\u{8}\u{c}")
        );
    }

    #[test]
    fn a_lone_surrogate_does_not_desynchronise_the_reader() {
        let body = "{\"a\": \"x\\ud800y\", \"b\": \"ok\"}";
        assert_eq!(
            extract_json_string_literal(body, "a").as_deref(),
            Some("x\u{fffd}y")
        );
        assert_eq!(extract_json_string_literal(body, "b").as_deref(), Some("ok"));
    }
}
