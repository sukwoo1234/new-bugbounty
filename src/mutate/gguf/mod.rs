#![allow(dead_code)]

use crate::mutate::common::DeterministicRng;

pub(crate) mod byte_flip;
pub(crate) mod header_counts;
pub(crate) mod metadata_key;
pub(crate) mod metadata_type;
pub(crate) mod metadata_value;
pub(crate) mod tensor_dtype;
pub(crate) mod tensor_name;
pub(crate) mod tensor_offset;
pub(crate) mod tensor_shape;

pub(crate) const MAGIC: &[u8; 4] = b"GGUF";
pub(crate) const SUPPORTED_VERSION: u32 = 3;
pub(crate) const DEFAULT_ALIGNMENT: u64 = 32;
pub(crate) const ALIGNMENT_KEY: &str = "general.alignment";

pub(crate) const DEFAULT_OPERATORS: &[&str] = &[];
pub(crate) const KNOWN_OPERATORS: &[&str] = &[];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    TooSmall,
    BadMagic,
    UnsupportedVersion(u32),
    Truncated(&'static str),
    InvalidValueType(u32),
    OversizedCount(&'static str),
    InvalidUtf8,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall => write!(f, "gguf input smaller than 24-byte header"),
            Self::BadMagic => write!(f, "gguf magic mismatch (expected 'GGUF')"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported gguf version {} (supported: 3)", v)
            }
            Self::Truncated(where_) => write!(f, "gguf input truncated at {}", where_),
            Self::InvalidValueType(v) => write!(f, "gguf invalid value_type {}", v),
            Self::OversizedCount(where_) => write!(f, "gguf oversized count at {}", where_),
            Self::InvalidUtf8 => write!(f, "gguf string is not valid utf-8"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GgufValueType {
    U8 = 0,
    I8 = 1,
    U16 = 2,
    I16 = 3,
    U32 = 4,
    I32 = 5,
    F32 = 6,
    Bool = 7,
    String = 8,
    Array = 9,
    U64 = 10,
    I64 = 11,
    F64 = 12,
}

impl GgufValueType {
    pub(crate) fn from_u32(n: u32) -> Option<Self> {
        match n {
            0 => Some(Self::U8),
            1 => Some(Self::I8),
            2 => Some(Self::U16),
            3 => Some(Self::I16),
            4 => Some(Self::U32),
            5 => Some(Self::I32),
            6 => Some(Self::F32),
            7 => Some(Self::Bool),
            8 => Some(Self::String),
            9 => Some(Self::Array),
            10 => Some(Self::U64),
            11 => Some(Self::I64),
            12 => Some(Self::F64),
            _ => None,
        }
    }

    pub(crate) fn scalar_size(self) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 | Self::Bool => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 | Self::F32 => Some(4),
            Self::U64 | Self::I64 | Self::F64 => Some(8),
            Self::String | Self::Array => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct KvEntry {
    pub(crate) entry_start: usize,
    pub(crate) entry_end: usize,
    pub(crate) key_str_start: usize,
    pub(crate) key_str_end: usize,
    pub(crate) value_type: GgufValueType,
    pub(crate) value_type_start: usize,
    pub(crate) value_payload_start: usize,
    pub(crate) value_payload_end: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TensorEntry {
    pub(crate) entry_start: usize,
    pub(crate) entry_end: usize,
    pub(crate) name_str_start: usize,
    pub(crate) name_str_end: usize,
    pub(crate) n_dims: u32,
    pub(crate) shape_start: usize,
    pub(crate) shape_end: usize,
    pub(crate) type_start: usize,
    pub(crate) offset_start: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct GgufLayout {
    pub(crate) version: u32,
    pub(crate) tensor_count: u64,
    pub(crate) kv_count: u64,
    pub(crate) alignment: u64,
    pub(crate) kvs: Vec<KvEntry>,
    pub(crate) tensors: Vec<TensorEntry>,
    pub(crate) tensor_data_start: usize,
}

pub(crate) fn parse_gguf(bytes: &[u8]) -> Result<GgufLayout, ParseError> {
    if bytes.len() < 24 {
        return Err(ParseError::TooSmall);
    }
    if &bytes[0..4] != MAGIC {
        return Err(ParseError::BadMagic);
    }
    let version = read_u32(bytes, 4)?;
    if version != SUPPORTED_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let tensor_count = read_u64(bytes, 8)?;
    let kv_count = read_u64(bytes, 16)?;

    let mut cursor = 24usize;
    let mut kvs = Vec::new();
    let mut alignment = DEFAULT_ALIGNMENT;
    for _ in 0..kv_count {
        let entry_start = cursor;
        let key_len = read_u64(bytes, cursor)?;
        cursor = cursor
            .checked_add(8)
            .ok_or(ParseError::Truncated("kv:key_len"))?;
        let key_len_usize = usize::try_from(key_len)
            .map_err(|_| ParseError::OversizedCount("kv:key_len"))?;
        let key_str_start = cursor;
        let key_str_end = cursor
            .checked_add(key_len_usize)
            .ok_or(ParseError::Truncated("kv:key_str"))?;
        if key_str_end > bytes.len() {
            return Err(ParseError::Truncated("kv:key_str"));
        }
        std::str::from_utf8(&bytes[key_str_start..key_str_end])
            .map_err(|_| ParseError::InvalidUtf8)?;
        cursor = key_str_end;
        let value_type_start = cursor;
        let value_type_raw = read_u32(bytes, cursor)?;
        let value_type = GgufValueType::from_u32(value_type_raw)
            .ok_or(ParseError::InvalidValueType(value_type_raw))?;
        cursor = cursor
            .checked_add(4)
            .ok_or(ParseError::Truncated("kv:value_type"))?;
        let value_payload_start = cursor;
        let value_payload_end = skip_value(bytes, cursor, value_type)?;
        cursor = value_payload_end;

        if value_type == GgufValueType::U32
            && &bytes[key_str_start..key_str_end] == ALIGNMENT_KEY.as_bytes()
        {
            let raw = read_u32(bytes, value_payload_start)? as u64;
            if raw > 0 {
                alignment = raw;
            }
        }

        kvs.push(KvEntry {
            entry_start,
            entry_end: value_payload_end,
            key_str_start,
            key_str_end,
            value_type,
            value_type_start,
            value_payload_start,
            value_payload_end,
        });
    }

    let mut tensors = Vec::new();
    for _ in 0..tensor_count {
        let entry_start = cursor;
        let name_len = read_u64(bytes, cursor)?;
        cursor = cursor
            .checked_add(8)
            .ok_or(ParseError::Truncated("tensor:name_len"))?;
        let name_len_usize = usize::try_from(name_len)
            .map_err(|_| ParseError::OversizedCount("tensor:name_len"))?;
        let name_str_start = cursor;
        let name_str_end = cursor
            .checked_add(name_len_usize)
            .ok_or(ParseError::Truncated("tensor:name_str"))?;
        if name_str_end > bytes.len() {
            return Err(ParseError::Truncated("tensor:name_str"));
        }
        std::str::from_utf8(&bytes[name_str_start..name_str_end])
            .map_err(|_| ParseError::InvalidUtf8)?;
        cursor = name_str_end;
        let n_dims = read_u32(bytes, cursor)?;
        cursor = cursor
            .checked_add(4)
            .ok_or(ParseError::Truncated("tensor:n_dims"))?;
        let shape_start = cursor;
        let shape_byte_len = (n_dims as usize)
            .checked_mul(8)
            .ok_or(ParseError::OversizedCount("tensor:shape_len"))?;
        let shape_end = cursor
            .checked_add(shape_byte_len)
            .ok_or(ParseError::Truncated("tensor:shape"))?;
        if shape_end > bytes.len() {
            return Err(ParseError::Truncated("tensor:shape"));
        }
        cursor = shape_end;
        let type_start = cursor;
        cursor = cursor
            .checked_add(4)
            .ok_or(ParseError::Truncated("tensor:type"))?;
        if cursor > bytes.len() {
            return Err(ParseError::Truncated("tensor:type"));
        }
        let offset_start = cursor;
        cursor = cursor
            .checked_add(8)
            .ok_or(ParseError::Truncated("tensor:offset"))?;
        if cursor > bytes.len() {
            return Err(ParseError::Truncated("tensor:offset"));
        }
        tensors.push(TensorEntry {
            entry_start,
            entry_end: cursor,
            name_str_start,
            name_str_end,
            n_dims,
            shape_start,
            shape_end,
            type_start,
            offset_start,
        });
    }

    let tensor_data_start = align_up(cursor, alignment as usize);

    Ok(GgufLayout {
        version,
        tensor_count,
        kv_count,
        alignment,
        kvs,
        tensors,
        tensor_data_start,
    })
}

fn skip_value(bytes: &[u8], start: usize, value_type: GgufValueType) -> Result<usize, ParseError> {
    if let Some(scalar) = value_type.scalar_size() {
        let end = start
            .checked_add(scalar)
            .ok_or(ParseError::Truncated("value:scalar"))?;
        if end > bytes.len() {
            return Err(ParseError::Truncated("value:scalar"));
        }
        return Ok(end);
    }
    match value_type {
        GgufValueType::String => {
            let len = read_u64(bytes, start)?;
            let after_len = start
                .checked_add(8)
                .ok_or(ParseError::Truncated("value:string_len"))?;
            let len_usize = usize::try_from(len)
                .map_err(|_| ParseError::OversizedCount("value:string_len"))?;
            let end = after_len
                .checked_add(len_usize)
                .ok_or(ParseError::Truncated("value:string"))?;
            if end > bytes.len() {
                return Err(ParseError::Truncated("value:string"));
            }
            std::str::from_utf8(&bytes[after_len..end]).map_err(|_| ParseError::InvalidUtf8)?;
            Ok(end)
        }
        GgufValueType::Array => {
            let elem_type_raw = read_u32(bytes, start)?;
            let elem_type = GgufValueType::from_u32(elem_type_raw)
                .ok_or(ParseError::InvalidValueType(elem_type_raw))?;
            let after_elem_type = start
                .checked_add(4)
                .ok_or(ParseError::Truncated("value:array_elem_type"))?;
            let count = read_u64(bytes, after_elem_type)?;
            let after_count = after_elem_type
                .checked_add(8)
                .ok_or(ParseError::Truncated("value:array_count"))?;
            let count_usize = usize::try_from(count)
                .map_err(|_| ParseError::OversizedCount("value:array_count"))?;
            let mut cursor = after_count;
            for _ in 0..count_usize {
                cursor = skip_value(bytes, cursor, elem_type)?;
            }
            Ok(cursor)
        }
        _ => Err(ParseError::InvalidValueType(value_type as u32)),
    }
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ParseError> {
    let end = offset.checked_add(4).ok_or(ParseError::Truncated("u32"))?;
    if end > bytes.len() {
        return Err(ParseError::Truncated("u32"));
    }
    let mut arr = [0u8; 4];
    arr.copy_from_slice(&bytes[offset..end]);
    Ok(u32::from_le_bytes(arr))
}

pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ParseError> {
    let end = offset.checked_add(8).ok_or(ParseError::Truncated("u64"))?;
    if end > bytes.len() {
        return Err(ParseError::Truncated("u64"));
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[offset..end]);
    Ok(u64::from_le_bytes(arr))
}

pub(crate) fn write_u32(out: &mut [u8], offset: usize, value: u32) {
    let bytes = value.to_le_bytes();
    out[offset..offset + 4].copy_from_slice(&bytes);
}

pub(crate) fn write_u64(out: &mut [u8], offset: usize, value: u64) {
    let bytes = value.to_le_bytes();
    out[offset..offset + 8].copy_from_slice(&bytes);
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

pub(crate) fn truncate_param_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

pub(crate) fn align_up(value: usize, alignment: usize) -> usize {
    if alignment <= 1 {
        return value;
    }
    let rem = value % alignment;
    if rem == 0 {
        value
    } else {
        value + (alignment - rem)
    }
}
