#![allow(dead_code)]

use crate::mutate::common::DeterministicRng;

pub(crate) mod header_length;
pub(crate) mod metadata_key;
pub(crate) mod metadata_value;
pub(crate) mod tensor_data_offsets;
pub(crate) mod tensor_dtype;
pub(crate) mod tensor_name;
pub(crate) mod tensor_shape;

pub(crate) const HEADER_LEN_BYTES: usize = 8;
pub(crate) const NATURAL_ALIGNMENT_SMALL: u64 = 8;
pub(crate) const NATURAL_ALIGNMENT_LARGE: u64 = 64;

pub(crate) const DEFAULT_OPERATORS: &[&str] = &[];
pub(crate) const KNOWN_OPERATORS: &[&str] = &[];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    TooSmall,
    HeaderLenZero,
    HeaderLenOverflow,
    HeaderOutOfRange,
    InvalidUtf8,
    JsonUnexpectedEof,
    JsonExpected(u8),
    JsonExpectedNumber,
    JsonExpectedString,
    JsonBadEscape,
    JsonRootNotObject,
    TensorMissingField(&'static str),
    DataOffsetsNotPair,
    ShapeNotArray,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall => write!(f, "safetensors smaller than 8-byte header prefix"),
            Self::HeaderLenZero => write!(f, "safetensors header length is zero"),
            Self::HeaderLenOverflow => write!(f, "safetensors header length overflow"),
            Self::HeaderOutOfRange => write!(f, "safetensors header out of range"),
            Self::InvalidUtf8 => write!(f, "safetensors header is not utf-8"),
            Self::JsonUnexpectedEof => write!(f, "safetensors json truncated"),
            Self::JsonExpected(b) => {
                write!(f, "safetensors json expected '{}'", *b as char)
            }
            Self::JsonExpectedNumber => write!(f, "safetensors json expected number"),
            Self::JsonExpectedString => write!(f, "safetensors json expected string"),
            Self::JsonBadEscape => write!(f, "safetensors json bad escape sequence"),
            Self::JsonRootNotObject => write!(f, "safetensors json root is not object"),
            Self::TensorMissingField(name) => {
                write!(f, "safetensors tensor missing field '{}'", name)
            }
            Self::DataOffsetsNotPair => {
                write!(f, "safetensors data_offsets is not a 2-element array")
            }
            Self::ShapeNotArray => write!(f, "safetensors shape is not an array"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StringSpan {
    pub(crate) outer_start: usize,
    pub(crate) outer_end: usize,
    pub(crate) inner_start: usize,
    pub(crate) inner_end: usize,
}

impl StringSpan {
    pub(crate) fn inner_len(&self) -> usize {
        self.inner_end - self.inner_start
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NumberSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl NumberSpan {
    pub(crate) fn len(&self) -> usize {
        self.end - self.start
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ArraySpan {
    pub(crate) open: usize,
    pub(crate) close: usize,
    pub(crate) elements: Vec<NumberSpan>,
}

#[derive(Debug, Clone)]
pub(crate) struct TensorEntry {
    pub(crate) name: StringSpan,
    pub(crate) dtype: StringSpan,
    pub(crate) shape: ArraySpan,
    pub(crate) data_offsets: ArraySpan,
}

#[derive(Debug, Clone)]
pub(crate) struct MetadataKv {
    pub(crate) key: StringSpan,
    pub(crate) value: StringSpan,
}

#[derive(Debug, Clone)]
pub(crate) struct MetadataSection {
    pub(crate) kvs: Vec<MetadataKv>,
}

#[derive(Debug, Clone)]
pub(crate) struct SafetensorsLayout {
    pub(crate) header_len: u64,
    pub(crate) json_start: usize,
    pub(crate) json_end: usize,
    pub(crate) header_end: usize,
    pub(crate) tensors: Vec<TensorEntry>,
    pub(crate) metadata: Option<MetadataSection>,
}

pub(crate) fn parse_safetensors(bytes: &[u8]) -> Result<SafetensorsLayout, ParseError> {
    if bytes.len() < HEADER_LEN_BYTES {
        return Err(ParseError::TooSmall);
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[0..HEADER_LEN_BYTES]);
    let header_len = u64::from_le_bytes(arr);
    if header_len == 0 {
        return Err(ParseError::HeaderLenZero);
    }
    let header_len_usize =
        usize::try_from(header_len).map_err(|_| ParseError::HeaderLenOverflow)?;
    let json_start = HEADER_LEN_BYTES;
    let json_end = HEADER_LEN_BYTES
        .checked_add(header_len_usize)
        .ok_or(ParseError::HeaderLenOverflow)?;
    if json_end > bytes.len() {
        return Err(ParseError::HeaderOutOfRange);
    }
    let json_slice = std::str::from_utf8(&bytes[json_start..json_end])
        .map_err(|_| ParseError::InvalidUtf8)?;

    let mut walker = JsonWalker::new(json_slice, json_start);
    walker.skip_whitespace();
    if walker.peek() != Some(b'{') {
        return Err(ParseError::JsonRootNotObject);
    }
    walker.cursor += 1;

    let mut tensors = Vec::new();
    let mut metadata: Option<MetadataSection> = None;
    let mut first = true;

    loop {
        walker.skip_whitespace();
        match walker.peek() {
            Some(b'}') => break,
            None => return Err(ParseError::JsonUnexpectedEof),
            _ => {}
        }
        if !first {
            walker.expect(b',')?;
            walker.skip_whitespace();
        }
        first = false;

        let key_span = walker.parse_string()?;
        let key_str = walker.slice_inner(&key_span).to_string();
        walker.skip_whitespace();
        walker.expect(b':')?;
        walker.skip_whitespace();

        if key_str == "__metadata__" {
            let section = walker.parse_metadata_object()?;
            metadata = Some(section);
        } else if walker.peek() == Some(b'{') {
            let entry = walker.parse_tensor_object(key_span)?;
            tensors.push(entry);
        } else {
            walker.skip_value()?;
        }
    }

    let header_end = json_end;

    Ok(SafetensorsLayout {
        header_len,
        json_start,
        json_end,
        header_end,
        tensors,
        metadata,
    })
}

struct JsonWalker<'a> {
    src: &'a str,
    bytes: &'a [u8],
    cursor: usize,
    base: usize,
}

impl<'a> JsonWalker<'a> {
    fn new(src: &'a str, base: usize) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            cursor: 0,
            base,
        }
    }

    fn abs(&self, relative: usize) -> usize {
        self.base + relative
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.cursor += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, ch: u8) -> Result<(), ParseError> {
        match self.peek() {
            Some(b) if b == ch => {
                self.cursor += 1;
                Ok(())
            }
            Some(_) => Err(ParseError::JsonExpected(ch)),
            None => Err(ParseError::JsonUnexpectedEof),
        }
    }

    fn slice_inner(&self, span: &StringSpan) -> &str {
        let s = span.inner_start - self.base;
        let e = span.inner_end - self.base;
        &self.src[s..e]
    }

    fn parse_string(&mut self) -> Result<StringSpan, ParseError> {
        if self.peek() != Some(b'"') {
            return Err(ParseError::JsonExpectedString);
        }
        let outer_start = self.cursor;
        self.cursor += 1;
        let inner_start = self.cursor;
        while let Some(b) = self.peek() {
            match b {
                b'\\' => {
                    self.cursor += 1;
                    if self.peek().is_none() {
                        return Err(ParseError::JsonBadEscape);
                    }
                    self.cursor += 1;
                }
                b'"' => {
                    let inner_end = self.cursor;
                    self.cursor += 1;
                    return Ok(StringSpan {
                        outer_start: self.abs(outer_start),
                        outer_end: self.abs(self.cursor),
                        inner_start: self.abs(inner_start),
                        inner_end: self.abs(inner_end),
                    });
                }
                _ => self.cursor += 1,
            }
        }
        Err(ParseError::JsonUnexpectedEof)
    }

    fn parse_number(&mut self) -> Result<NumberSpan, ParseError> {
        let start = self.cursor;
        if matches!(self.peek(), Some(b'-')) {
            self.cursor += 1;
        }
        let digits_start = self.cursor;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.cursor += 1;
            } else {
                break;
            }
        }
        if self.cursor == digits_start {
            return Err(ParseError::JsonExpectedNumber);
        }
        if matches!(self.peek(), Some(b'.')) {
            self.cursor += 1;
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.cursor += 1;
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.cursor += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.cursor += 1;
            }
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.cursor += 1;
                } else {
                    break;
                }
            }
        }
        Ok(NumberSpan {
            start: self.abs(start),
            end: self.abs(self.cursor),
        })
    }

    fn parse_number_array(&mut self) -> Result<ArraySpan, ParseError> {
        self.expect(b'[')?;
        let open = self.abs(self.cursor - 1);
        let mut elements = Vec::new();
        self.skip_whitespace();
        let mut first = true;
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b']') {
                self.cursor += 1;
                let close = self.abs(self.cursor);
                return Ok(ArraySpan {
                    open,
                    close,
                    elements,
                });
            }
            if !first {
                self.expect(b',')?;
                self.skip_whitespace();
            }
            first = false;
            let num = self.parse_number()?;
            elements.push(num);
        }
    }

    fn parse_tensor_object(&mut self, name: StringSpan) -> Result<TensorEntry, ParseError> {
        self.expect(b'{')?;
        let mut dtype: Option<StringSpan> = None;
        let mut shape: Option<ArraySpan> = None;
        let mut data_offsets: Option<ArraySpan> = None;
        let mut first = true;
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b'}') {
                self.cursor += 1;
                break;
            }
            if !first {
                self.expect(b',')?;
                self.skip_whitespace();
            }
            first = false;
            let field_key = self.parse_string()?;
            let key_str = self.slice_inner(&field_key).to_string();
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            match key_str.as_str() {
                "dtype" => {
                    let span = self.parse_string()?;
                    dtype = Some(span);
                }
                "shape" => {
                    if self.peek() != Some(b'[') {
                        return Err(ParseError::ShapeNotArray);
                    }
                    let arr = self.parse_number_array()?;
                    shape = Some(arr);
                }
                "data_offsets" => {
                    if self.peek() != Some(b'[') {
                        return Err(ParseError::DataOffsetsNotPair);
                    }
                    let arr = self.parse_number_array()?;
                    data_offsets = Some(arr);
                }
                _ => {
                    self.skip_value()?;
                }
            }
        }
        let dtype = dtype.ok_or(ParseError::TensorMissingField("dtype"))?;
        let shape = shape.ok_or(ParseError::TensorMissingField("shape"))?;
        let data_offsets =
            data_offsets.ok_or(ParseError::TensorMissingField("data_offsets"))?;
        if data_offsets.elements.len() != 2 {
            return Err(ParseError::DataOffsetsNotPair);
        }
        Ok(TensorEntry {
            name,
            dtype,
            shape,
            data_offsets,
        })
    }

    fn parse_metadata_object(&mut self) -> Result<MetadataSection, ParseError> {
        self.expect(b'{')?;
        let mut kvs = Vec::new();
        let mut first = true;
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b'}') {
                self.cursor += 1;
                break;
            }
            if !first {
                self.expect(b',')?;
                self.skip_whitespace();
            }
            first = false;
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            if self.peek() == Some(b'"') {
                let value = self.parse_string()?;
                kvs.push(MetadataKv { key, value });
            } else {
                self.skip_value()?;
            }
        }
        Ok(MetadataSection { kvs })
    }

    fn skip_value(&mut self) -> Result<(), ParseError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => {
                self.parse_string()?;
                Ok(())
            }
            Some(b'{') => {
                self.cursor += 1;
                let mut first = true;
                loop {
                    self.skip_whitespace();
                    if self.peek() == Some(b'}') {
                        self.cursor += 1;
                        return Ok(());
                    }
                    if !first {
                        self.expect(b',')?;
                        self.skip_whitespace();
                    }
                    first = false;
                    let _ = self.parse_string()?;
                    self.skip_whitespace();
                    self.expect(b':')?;
                    self.skip_value()?;
                }
            }
            Some(b'[') => {
                self.cursor += 1;
                let mut first = true;
                loop {
                    self.skip_whitespace();
                    if self.peek() == Some(b']') {
                        self.cursor += 1;
                        return Ok(());
                    }
                    if !first {
                        self.expect(b',')?;
                        self.skip_whitespace();
                    }
                    first = false;
                    self.skip_value()?;
                }
            }
            Some(b't') | Some(b'f') | Some(b'n') => {
                while let Some(b) = self.peek() {
                    if b.is_ascii_alphabetic() {
                        self.cursor += 1;
                    } else {
                        break;
                    }
                }
                Ok(())
            }
            Some(_) => {
                self.parse_number()?;
                Ok(())
            }
            None => Err(ParseError::JsonUnexpectedEof),
        }
    }
}

pub(crate) fn pick_different_ascii_byte(rng: &mut DeterministicRng, current: u8) -> u8 {
    let range = (0x7eu8 - 0x21u8 + 1) as usize;
    loop {
        let candidate = 0x21u8 + (rng.index(range) as u8);
        if candidate != current {
            return candidate;
        }
    }
}

pub(crate) fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64, ParseError> {
    let end = offset.checked_add(8).ok_or(ParseError::HeaderLenOverflow)?;
    if end > bytes.len() {
        return Err(ParseError::TooSmall);
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[offset..end]);
    Ok(u64::from_le_bytes(arr))
}

pub(crate) fn write_u64_le(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn natural_alignment_of(offset: u64) -> u64 {
    if offset == 0 || offset % NATURAL_ALIGNMENT_LARGE == 0 {
        NATURAL_ALIGNMENT_LARGE
    } else if offset % NATURAL_ALIGNMENT_SMALL == 0 {
        NATURAL_ALIGNMENT_SMALL
    } else {
        1
    }
}

pub(crate) fn digit_count_u64(value: u64) -> usize {
    let mut n = value;
    if n == 0 {
        return 1;
    }
    let mut c = 0;
    while n > 0 {
        c += 1;
        n /= 10;
    }
    c
}
