#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
SEED_ROOT="${SEED_ROOT:-$PROJECT_ROOT/seeds}"

usage() {
  cat <<'EOF'
usage: gen_gguf_malformed_seeds.sh [--verify]

Deterministically builds the minimal GGUF files that reach llama.cpp's parser
asserts, plus the well-formed control file. Seeds are gitignored, so the bytes
travel as this generator rather than as committed binaries.

  (no args)  write the seeds under SEED_ROOT and print sha256 + size
  --verify   rebuild into a temporary directory and compare against the
             sha256 constants below; exit 1 on any mismatch

Layout (GGUF v3):
  magic "GGUF" | u32 version=3 | u64 n_tensors | u64 n_kv
  KV: u64 key_len | key bytes | u32 value_type | value
  ARRAY value: u32 elem_type | u64 n | elements
  (gguf_type enum: ggml/include/gguf.h)

Files:
  gguf-malformed/align_wrongtype.gguf  general.alignment as UINT64 -> get_val<uint32_t> type assert
  gguf-malformed/align_array2.gguf     general.alignment as UINT32[2] -> get_ne() == 1 assert
  gguf-malformed/emptykey.gguf         zero-length KV key -> !key.empty() assert
  gguf/align_ok.gguf                   general.alignment = UINT32 32, parses cleanly

Environment:
  PROJECT_ROOT (default: cwd)
  SEED_ROOT    (default: $PROJECT_ROOT/seeds)
EOF
}

MODE=generate
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --verify) MODE=verify; shift ;;
    *) echo "[gen-gguf-seeds] unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# relative path <TAB> sha256 <TAB> size in bytes
EXPECTED=$(cat <<'EOF'
gguf-malformed/align_wrongtype.gguf	a3f966767f753a06074499ebe5161893d2051ac455ccc538453f278ee4abe307	61
gguf-malformed/align_array2.gguf	755cf48736502767302111da442997dbc8cb2c16f479219387478de709458989	73
gguf-malformed/emptykey.gguf	fe9b7e916d1a343aa1a116dfdbcf8d1c3f4f9fa08f8e2e9b300a075acd071c38	40
gguf/align_ok.gguf	364fe85fad1c2ab844e77f2e032e52cc5e4e270afa42b010f84dd66f415aa8ed	57
EOF
)

log() {
  echo "[gen-gguf-seeds] $*"
}

emit_seeds() {
  local out_root="$1"
  python3 - "$out_root" <<'PY'
import os
import struct
import sys

out_root = sys.argv[1]

MAGIC = b"GGUF"
VERSION = 3

# ggml/include/gguf.h
GGUF_TYPE_UINT32 = 4
GGUF_TYPE_ARRAY = 9
GGUF_TYPE_UINT64 = 10

ALIGN_KEY = b"general.alignment"
ALIGN_VALUE = 32


def header(n_tensors, n_kv):
    return MAGIC + struct.pack("<IQQ", VERSION, n_tensors, n_kv)


def kv(key, value_type, payload):
    return struct.pack("<Q", len(key)) + key + struct.pack("<I", value_type) + payload


def kv_array(key, elem_type, elements):
    body = struct.pack("<IQ", elem_type, len(elements)) + b"".join(elements)
    return kv(key, GGUF_TYPE_ARRAY, body)


u32 = lambda v: struct.pack("<I", v)

seeds = [
    # general.alignment declared UINT64: gguf_get_val_u32 asserts on the type mismatch.
    (
        "gguf-malformed/align_wrongtype.gguf",
        header(0, 1) + kv(ALIGN_KEY, GGUF_TYPE_UINT64, struct.pack("<Q", ALIGN_VALUE)),
    ),
    # general.alignment as a 2-element array: gguf_get_val_u32 asserts get_ne() == 1.
    (
        "gguf-malformed/align_array2.gguf",
        header(0, 1) + kv_array(ALIGN_KEY, GGUF_TYPE_UINT32, [u32(ALIGN_VALUE), u32(ALIGN_VALUE)]),
    ),
    # zero-length key: the gguf_kv constructor asserts !key.empty().
    (
        "gguf-malformed/emptykey.gguf",
        header(0, 1) + kv(b"", GGUF_TYPE_UINT32, u32(ALIGN_VALUE)),
    ),
    # control: the same shape, well formed.
    (
        "gguf/align_ok.gguf",
        header(0, 1) + kv(ALIGN_KEY, GGUF_TYPE_UINT32, u32(ALIGN_VALUE)),
    ),
]

for rel, data in seeds:
    path = os.path.join(out_root, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as fh:
        fh.write(data)
PY
}

sha256_of() {
  sha256sum "$1" | cut -d' ' -f1
}

if [[ "$MODE" == generate ]]; then
  emit_seeds "$SEED_ROOT"
  status=0
  while IFS=$'\t' read -r rel expected_sha expected_size; do
    [[ -n "$rel" ]] || continue
    path="$SEED_ROOT/$rel"
    actual_sha="$(sha256_of "$path")"
    actual_size="$(stat -c %s "$path")"
    log "$rel sha256=$actual_sha size=$actual_size"
    if [[ "$actual_size" != "$expected_size" ]]; then
      log "SIZE MISMATCH $rel expected=$expected_size actual=$actual_size"
      status=1
    fi
    if [[ "$expected_sha" != PLACEHOLDER_* && "$actual_sha" != "$expected_sha" ]]; then
      log "SHA MISMATCH $rel expected=$expected_sha actual=$actual_sha"
      status=1
    fi
  done <<< "$EXPECTED"
  exit "$status"
fi

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT
emit_seeds "$tmp_root"

status=0
while IFS=$'\t' read -r rel expected_sha expected_size; do
  [[ -n "$rel" ]] || continue
  path="$tmp_root/$rel"
  if [[ ! -f "$path" ]]; then
    log "MISSING $rel"
    status=1
    continue
  fi
  actual_sha="$(sha256_of "$path")"
  actual_size="$(stat -c %s "$path")"
  if [[ "$actual_sha" != "$expected_sha" || "$actual_size" != "$expected_size" ]]; then
    log "MISMATCH $rel expected=$expected_sha/$expected_size actual=$actual_sha/$actual_size"
    status=1
  else
    log "OK $rel sha256=$actual_sha size=$actual_size"
  fi
done <<< "$EXPECTED"

if [[ "$status" -ne 0 ]]; then
  log "verify FAILED"
else
  log "verify OK"
fi
exit "$status"
