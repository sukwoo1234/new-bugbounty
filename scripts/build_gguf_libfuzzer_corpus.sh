#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
SEED_DIR="${SEED_DIR:-$PROJECT_ROOT/seeds/gguf}"
OUT_DIR="${OUT_DIR:-$PROJECT_ROOT/data/corpus/gguf-libfuzzer}"
REPLAY_BIN="${REPLAY_BIN:-$PROJECT_ROOT/harnesses/libfuzzer/gguf_loader_replay}"
# libFuzzer truncates the inputs it generates; a seed above that cap is read but never
# reproduced, so the mutants stay smaller than the seed and the deep paths go unvisited.
MAX_BYTES="${MAX_BYTES:-1048576}"
# Build to just under the cap: the biggest file that still fits keeps the most coverage.
TARGET_BYTES="${TARGET_BYTES:-1000000}"

usage() {
  cat <<'EOF'
usage: build_gguf_libfuzzer_corpus.sh [--verify]

Builds a libFuzzer-sized GGUF seed corpus by shrinking seeds/gguf in structure-
preserving ways, so every derived seed stays under MAX_BYTES (1 MiB by default).

Why: libFuzzer never generates an input longer than -max_len, and our loops do not pass
-max_len, so it derives one from the corpus and does not go above 1 MiB. 15 of the 19
gguf seeds are larger than that (1.16 MB - 10.9 MB; the other 4 are 57 B - 726 KB and
pass through untouched). Those 15 are read, but nothing libFuzzer generates ever reaches
their size, so the deep structure they were chosen for is never reproduced. ONNX never
hit this - 3 of its 33 seeds are over the cap - so it is a format difference that
reaches all the way into corpus strategy.

What actually shrinks: NOT the tensor data. 18 of the 19 seeds (17 ggml-vocab-* files
plus the generated align_ok.gguf) have n_tensors=0 and no data section at all - their
megabytes are metadata (the tokenizer token/score/merge arrays). Only
stories260K-f32.gguf has tensors. So the reducer:
  1. caps the element count of metadata ARRAYS (descending ladder until under target),
  2. keeps the longest prefix of the tensors, in offset order, that still fits under
     TARGET_BYTES and truncates the data section at the cut - ggml requires each offset
     to equal the running sum of padded sizes (gguf.cpp:634), and an offset-ordered
     prefix preserves that without recomputing a single offset. Only stories260K has
     tensors at all; it keeps 37 of its 48.
Every key survives, so no key-driven path is lost; only the values get shorter.

  (no args)  write the derived corpus into OUT_DIR
  --verify   check every file in OUT_DIR: under MAX_BYTES, still parses, keeps the full
             key set of its seed, and replays through the native harness with exit 0

Note the two directories are different things:
  OUT_DIR (data/corpus/gguf-libfuzzer)  read-only derived SEEDS, this script's output
  data/corpus/libfuzzer/gguf            libFuzzer's writable working corpus, seeded from
                                        the above by ops/scripts/fuzz-loop-libfuzzer.sh

Environment:
  PROJECT_ROOT SEED_DIR OUT_DIR REPLAY_BIN MAX_BYTES TARGET_BYTES
  ALLOW_MISSING_REPLAY=1  let --verify pass without the native replay binary
EOF
}

MODE=build
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --verify) MODE=verify; shift ;;
    *) echo "[gguf-libfuzzer-corpus] unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

log() { echo "[gguf-libfuzzer-corpus] $*"; }
fail() { echo "[gguf-libfuzzer-corpus] fail: $*" >&2; exit 1; }

[[ -d "$SEED_DIR" ]] || fail "seed dir not found: $SEED_DIR"
# Writing the reduced files back over the fixture would destroy the 19-file seed set the
# oracle check asserts on, and /seeds is gitignored so there is no copy to restore from.
if [[ "$(readlink -f "$OUT_DIR" 2>/dev/null || echo "$OUT_DIR")" == "$(readlink -f "$SEED_DIR")" ]]; then
  fail "OUT_DIR must not be SEED_DIR ($SEED_DIR): that would overwrite the seed fixture"
fi

reduce_py() {
  python3 - "$1" "$SEED_DIR" "$OUT_DIR" "$TARGET_BYTES" "$MAX_BYTES" <<'PY'
import os
import struct
import sys

mode, seed_dir, out_dir, target, max_bytes = (
    sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
)

# ggml/include/gguf.h
SCALAR = {0: 1, 1: 1, 2: 2, 3: 2, 4: 4, 5: 4, 6: 4, 7: 1, 10: 8, 11: 8, 12: 8}
STRING, ARRAY = 8, 9
ALIGN_KEY = b"general.alignment"
ARRAY_CAPS = [None, 65536, 16384, 4096, 1024, 256, 64, 16, 4, 1, 0]


class Bad(Exception):
    pass


def skip_value(b, o, t):
    if t in SCALAR:
        return o + SCALAR[t]
    if t == STRING:
        (n,) = struct.unpack_from("<Q", b, o)
        return o + 8 + n
    if t == ARRAY:
        et, n = struct.unpack_from("<IQ", b, o)
        o += 12
        if et in SCALAR:
            return o + n * SCALAR[et]
        for _ in range(n):
            o = skip_value(b, o, et)
        return o
    raise Bad("value type %d" % t)


def parse(b):
    if len(b) < 24 or b[:4] != b"GGUF":
        raise Bad("not a gguf file")
    version, n_tensors, n_kv = struct.unpack_from("<IQQ", b, 4)
    if version not in (2, 3):
        # ggml rejects v1 and anything above v3 and parses the rest identically
        # (gguf.cpp:353-378); aquila is a v2 file the harness loads cleanly.
        raise Bad("version %d" % version)
    o = 24
    kvs = []
    alignment = 32
    for _ in range(n_kv):
        (kl,) = struct.unpack_from("<Q", b, o)
        key = b[o + 8 : o + 8 + kl]
        o += 8 + kl
        (t,) = struct.unpack_from("<I", b, o)
        o += 4
        start = o
        o = skip_value(b, o, t)
        if o > len(b):
            raise Bad("truncated value for key %r" % key)
        if key == ALIGN_KEY and t == 4:
            (raw,) = struct.unpack_from("<I", b, start)
            if raw > 0:
                alignment = raw
        kvs.append((key, t, b[start:o]))
    tensors = []
    for _ in range(n_tensors):
        (nl,) = struct.unpack_from("<Q", b, o)
        name = b[o + 8 : o + 8 + nl]
        o += 8 + nl
        (nd,) = struct.unpack_from("<I", b, o)
        o += 4
        dims = list(struct.unpack_from("<%dQ" % nd, b, o)) if nd else []
        o += 8 * nd
        (tt,) = struct.unpack_from("<I", b, o)
        o += 4
        (off,) = struct.unpack_from("<Q", b, o)
        o += 8
        tensors.append((name, dims, tt, off))
    if o > len(b):
        raise Bad("truncated tensor info")
    data_start = (o + alignment - 1) // alignment * alignment
    return version, kvs, tensors, b[data_start:], alignment


def build(version, kvs, tensors, data, alignment):
    out = bytearray(b"GGUF")
    out += struct.pack("<IQQ", version, len(tensors), len(kvs))
    for key, t, payload in kvs:
        out += struct.pack("<Q", len(key)) + key + struct.pack("<I", t) + payload
    for name, dims, tt, off in tensors:
        out += struct.pack("<Q", len(name)) + name + struct.pack("<I", len(dims))
        for d in dims:
            out += struct.pack("<Q", d)
        out += struct.pack("<I", tt) + struct.pack("<Q", off)
    # Only a file that HAS a data section needs the padding that aligns it. Padding a
    # file with no tensors just made every already-small seed a few bytes bigger than
    # the seed it came from, for nothing.
    if data:
        out += b"\0" * (-len(out) % alignment)
        out += data
    return bytes(out)


def cap_array(payload, cap):
    et, n = struct.unpack_from("<IQ", payload, 0)
    if n <= cap:
        return payload
    o = 12
    if et in SCALAR:
        end = o + cap * SCALAR[et]
    else:
        end = o
        for _ in range(cap):
            end = skip_value(payload, end, et)
    return struct.pack("<IQ", et, cap) + payload[o:end]


def tensor_cuts(tensors, data):
    """Tensor prefixes in offset order, and where the data section ends for each.

    ggml checks every offset against the running sum of padded tensor sizes
    (gguf.cpp:634), so a prefix taken in offset order keeps each surviving offset
    exactly right - no offset has to be recomputed, and the data is just cut short."""
    if not tensors:
        return [], [len(data)]
    ordered = sorted(tensors, key=lambda t: t[3])
    if [t[3] for t in ordered] != [t[3] for t in tensors]:
        # Offsets are not already ascending: leave this file's tensors alone rather
        # than reorder a layout we do not understand.
        return tensors, None
    cuts = [ordered[i][3] for i in range(len(ordered))] + [len(data)]
    return ordered, cuts


def reduce_file(raw):
    version, kvs, tensors, data, alignment = parse(raw)
    ordered, cuts = tensor_cuts(tensors, data)
    if cuts is None:
        # unusual offset order: keep the tensors as they are and shrink metadata only
        for cap in ARRAY_CAPS:
            out = build(version, apply_cap(kvs, cap), tensors, data, alignment)
            if len(out) <= target:
                return out, cap, len(tensors)
        return out, ARRAY_CAPS[-1], len(tensors)

    fallback = None
    for cap in ARRAY_CAPS:
        trimmed = apply_cap(kvs, cap)
        # Largest tensor prefix that still fits. Dropping a tensor drops its info bytes
        # as well as its data, so this has to be measured, not estimated.
        for k in range(len(ordered), -1, -1):
            out = build(version, trimmed, ordered[:k], data[: cuts[k]], alignment)
            if len(out) <= target:
                break
        else:
            continue
        if k > 0 or not ordered:
            return out, cap, k
        # Metadata alone leaves no room for even one tensor: shrink the arrays further
        # so the tensor-info and blob stages keep some coverage.
        fallback = fallback or (out, cap, k)
    if fallback:
        return fallback
    smallest = apply_cap(kvs, ARRAY_CAPS[-1])
    return build(version, smallest, [], b"", alignment), ARRAY_CAPS[-1], 0


def apply_cap(kvs, cap):
    if cap is None:
        return kvs
    return [(k, t, cap_array(p, cap) if t == ARRAY else p) for k, t, p in kvs]


def key_set(b):
    return sorted(k for k, _, _ in parse(b)[1])


def tensor_count(b):
    return len(parse(b)[2])


names = sorted(n for n in os.listdir(seed_dir) if n.endswith(".gguf"))
if not names:
    print("no .gguf seeds in %s" % seed_dir, file=sys.stderr)
    sys.exit(1)

status = 0
if mode == "build":
    os.makedirs(out_dir, exist_ok=True)
    for name in names:
        raw = open(os.path.join(seed_dir, name), "rb").read()
        try:
            out, cap, n_tensors = reduce_file(raw)
        except (Bad, struct.error) as e:
            print("SKIP %s: %s" % (name, e))
            status = 1
            continue
        if len(out) >= max_bytes:
            print("OVER %s: %d bytes even at the smallest array cap" % (name, len(out)))
            status = 1
            continue
        if key_set(out) != key_set(raw):
            print("KEYLOSS %s: the reduced file lost a metadata key" % name)
            status = 1
            continue
        if tensor_count(raw) and not n_tensors:
            # A derived seed with no tensors cannot exercise the tensor-info or blob
            # stages of the harness at all. Only stories260K has tensors, so losing them
            # would silently remove the whole of that coverage from the corpus.
            print("TENSORLOSS %s: every tensor was dropped (%d in the seed)"
                  % (name, tensor_count(raw)))
            status = 1
            continue
        with open(os.path.join(out_dir, name), "wb") as fh:
            fh.write(out)
        print(
            "%-32s %9d -> %8d  array_cap=%-6s tensors=%d"
            % (name, len(raw), len(out), "none" if cap is None else cap, n_tensors)
        )
else:
    produced = sorted(n for n in os.listdir(out_dir) if n.endswith(".gguf")) if os.path.isdir(out_dir) else []
    if produced != names:
        missing = set(names) - set(produced)
        extra = set(produced) - set(names)
        print("MISMATCH corpus does not cover the seed set; missing=%s extra=%s"
              % (sorted(missing), sorted(extra)))
        sys.exit(1)
    for name in names:
        raw = open(os.path.join(seed_dir, name), "rb").read()
        out = open(os.path.join(out_dir, name), "rb").read()
        if len(out) >= max_bytes:
            print("OVER %s: %d >= %d" % (name, len(out), max_bytes))
            status = 1
            continue
        try:
            if key_set(out) != key_set(raw):
                print("KEYLOSS %s" % name)
                status = 1
                continue
            seed_tensors, out_tensors = tensor_count(raw), tensor_count(out)
            if seed_tensors and not out_tensors:
                print("TENSORLOSS %s: seed had %d tensors, derived file has none"
                      % (name, seed_tensors))
                status = 1
                continue
        except (Bad, struct.error) as e:
            print("UNPARSABLE %s: %s" % (name, e))
            status = 1
            continue
        print("ok %-32s %8d bytes  tensors=%d" % (name, len(out), out_tensors))
sys.exit(status)
PY
}

if [[ "$MODE" == build ]]; then
  log "seeds=$SEED_DIR out=$OUT_DIR target=$TARGET_BYTES cap=$MAX_BYTES"
  reduce_py build || fail "reduction failed"
  log "built $(find "$OUT_DIR" -name '*.gguf' | wc -l | tr -d ' ') derived seeds"
  log "now run: $0 --verify"
  exit 0
fi

log "verifying $OUT_DIR against $SEED_DIR"
reduce_py verify || fail "structural verification failed"

if [[ ! -x "$REPLAY_BIN" ]]; then
  if [[ "${ALLOW_MISSING_REPLAY:-0}" == "1" ]]; then
    log "WARN replay binary missing and ALLOW_MISSING_REPLAY=1: the exit-0 half of this check did NOT run"
  else
    fail "replay binary not found: $REPLAY_BIN (build it with scripts/build_libfuzzer_gguf_native.sh, or set ALLOW_MISSING_REPLAY=1 to skip - which leaves the check unproven)"
  fi
else
  replayed=0
  while IFS= read -r f; do
    set +e
    timeout 120 "$REPLAY_BIN" "$f" >/dev/null 2>&1
    rc=$?
    set -e
    [[ "$rc" -eq 0 ]] || fail "replay exit $rc on $f (a derived seed must load cleanly)"
    replayed=$((replayed + 1))
  done < <(find "$OUT_DIR" -name '*.gguf' | sort)
  log "replay exit 0 on all $replayed derived seeds"
fi
log "verify OK"
