#!/usr/bin/env bash
# Deterministically generate safetensors seeds the shipped corpus lacks. seeds/ is
# gitignored, so this script (not the files) is the committed source of truth.
#
# Two sets:
#   seeds/safetensors/           - VALID but diverse: __metadata__-bearing and
#                                  non-F32 dtypes (BF16/BOOL/I8/mixed), so the
#                                  metadata_* and tensor_dtype operators actually fire
#                                  (the 26 shipped seeds are all F32 with no metadata).
#   seeds/safetensors-malformed/ - KNOWN-bad: the crate rejects these with a specific
#                                  SafeTensorError. Used as engine-check PoCs and to
#                                  exercise reject paths.
# Fully deterministic: no randomness, stable bytes across runs.
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
VALID_DIR="${VALID_DIR:-$PROJECT_ROOT/seeds/safetensors}"
MAL_DIR="${MAL_DIR:-$PROJECT_ROOT/seeds/safetensors-malformed}"
mkdir -p "$VALID_DIR" "$MAL_DIR"

python3 - "$VALID_DIR" "$MAL_DIR" <<'PY'
import json, struct, sys, os

valid_dir, mal_dir = sys.argv[1], sys.argv[2]

def build(header_obj, blob=b""):
    """A well-formed safetensors: 8-byte LE header length + JSON header + data blob."""
    hj = json.dumps(header_obj, separators=(",", ":")).encode("utf-8")
    return struct.pack("<Q", len(hj)) + hj + blob

def write(d, name, data):
    path = os.path.join(d, name)
    with open(path, "wb") as f:
        f.write(data)
    return path

# ---- VALID, diverse (into seeds/safetensors/, prefixed gen_ so they are obvious) ----
# metadata-bearing (so metadata_key/metadata_value stop returning NoApplicableField)
write(valid_dir, "gen_metadata.safetensors", build(
    {"__metadata__": {"format": "pt", "note": "gen"},
     "t0": {"dtype": "F32", "shape": [1], "data_offsets": [0, 4]}},
    b"\x00" * 4))
# BF16 (len-4 dtype group: BF16/BOOL) -> tensor_dtype exercises the len-4 bucket
write(valid_dir, "gen_bf16.safetensors", build(
    {"w": {"dtype": "BF16", "shape": [2], "data_offsets": [0, 4]}}, b"\x00" * 4))
# BOOL (len-4 group), 1 byte per element
write(valid_dir, "gen_bool.safetensors", build(
    {"w": {"dtype": "BOOL", "shape": [2], "data_offsets": [0, 2]}}, b"\x00" * 2))
# I8 (len-2 dtype group: I8/U8)
write(valid_dir, "gen_i8.safetensors", build(
    {"w": {"dtype": "I8", "shape": [3], "data_offsets": [0, 3]}}, b"\x00" * 3))
# mixed dtypes + metadata, contiguous offsets
write(valid_dir, "gen_mixed.safetensors", build(
    {"__metadata__": {"k": "v"},
     "a": {"dtype": "F16", "shape": [2], "data_offsets": [0, 4]},
     "b": {"dtype": "I16", "shape": [2], "data_offsets": [4, 8]}},
    b"\x00" * 8))

# ---- MALFORMED (into seeds/safetensors-malformed/), each a specific reject path ----
# 1. header length larger than the file (HeaderTooLarge / out of range)
hj = b'{"t":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}'
write(mal_dir, "header_len_huge.safetensors",
      struct.pack("<Q", 1 << 40) + hj + b"\x00" * 4)
# 2. header length points a few bytes past the actual header
write(mal_dir, "header_len_oob.safetensors",
      struct.pack("<Q", len(hj) + 32) + hj + b"\x00" * 4)
# 3. header is not valid UTF-8 (InvalidHeader)
bad = b"\xff\xfe\x00\x01not utf8 \xc3\x28"
write(mal_dir, "header_non_utf8.safetensors",
      struct.pack("<Q", len(bad)) + bad)
# 4. JSON root is an array, not an object (InvalidHeaderDeserialization)
arr = b"[1,2,3]"
write(mal_dir, "root_not_object.safetensors",
      struct.pack("<Q", len(arr)) + arr)
# 5. overlapping data_offsets (InvalidOffset)
write(mal_dir, "offsets_overlap.safetensors", build(
    {"a": {"dtype": "F32", "shape": [2], "data_offsets": [0, 8]},
     "b": {"dtype": "F32", "shape": [2], "data_offsets": [4, 12]}},
    b"\x00" * 12))
# 6. data_offsets end past the buffer (MetadataIncompleteBuffer / TensorInvalidInfo)
write(mal_dir, "offsets_oob.safetensors", build(
    {"t": {"dtype": "F32", "shape": [1], "data_offsets": [0, 4096]}}, b"\x00" * 4))
# 7. huge shape product -> ValidationOverflow
write(mal_dir, "shape_overflow.safetensors", build(
    {"t": {"dtype": "F32", "shape": [4294967296, 4294967296], "data_offsets": [0, 4]}},
    b"\x00" * 4))
# 8. deeply nested JSON in a value (our parser caps at 64; the crate's serde_json also
#    rejects) -> exercises the nesting-depth reject path
deep = '{"t":' + "[" * 200 + "]" * 200 + "}"
deepb = deep.encode("utf-8")
write(mal_dir, "deep_nesting.safetensors",
      struct.pack("<Q", len(deepb)) + deepb)
# 9. unknown dtype string (InvalidHeaderDeserialization on the Dtype enum)
write(mal_dir, "dtype_unknown.safetensors", build(
    {"t": {"dtype": "F99", "shape": [1], "data_offsets": [0, 4]}}, b"\x00" * 4))

n_valid = len([f for f in os.listdir(valid_dir) if f.startswith("gen_")])
n_mal = len(os.listdir(mal_dir))
print(f"[gen-st-seeds] valid gen_ seeds: {n_valid}  malformed seeds: {n_mal}")
PY
