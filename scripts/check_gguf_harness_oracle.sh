#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
REPLAY="${REPLAY:-$PROJECT_ROOT/harnesses/libfuzzer/gguf_loader_replay}"
LEGACY_PROBE="${LEGACY_PROBE:-$PROJECT_ROOT/tools/llama.cpp/build/bin/llama-gguf-hash}"
SEED_DIR="${SEED_DIR:-$PROJECT_ROOT/seeds/gguf}"
MALFORMED_DIR="${MALFORMED_DIR:-$PROJECT_ROOT/seeds/gguf-malformed}"
OUT_DIR="${OUT_DIR:-$PROJECT_ROOT/data/native-engine-checks/gguf}"
REQUIRE_NATIVE=0

usage() {
  cat <<'EOF'
usage: check_gguf_harness_oracle.sh [--require-native]

Proves the native GGUF replay harness answers the parser honestly:

  well-formed seeds                 -> exit 0
  files the parser rejects          -> exit 9   (NOT a crash)
  files that trip a GGML_ASSERT     -> death by signal (SIGABRT = 134)

The reject cases are the point: llama-gguf-hash dereferences the NULL that
gguf_init_from_file returns, so it dies with SIGSEGV on files the parser
deliberately turned down. This check records that contrast.

Without --require-native, a missing replay binary is a skip (exit 0) so the
check can sit in an offline suite. With it, the missing binary is a failure.

Environment:
  PROJECT_ROOT   (default: cwd)
  REPLAY         native standalone replay binary
  LEGACY_PROBE   llama-gguf-hash, for the contrast assertion (optional)
  SEED_DIR / MALFORMED_DIR / OUT_DIR
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --require-native) REQUIRE_NATIVE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[gguf-oracle] unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

log() {
  echo "[gguf-oracle] $*"
}

fail() {
  echo "[gguf-oracle] fail: $*" >&2
  exit 1
}

FAILURES=0

check() {
  # check <label> <expected> <actual>
  local label="$1" expected="$2" actual="$3"
  if [[ "$actual" == "$expected" ]]; then
    log "OK   $label rc=$actual"
  else
    log "FAIL $label expected rc=$expected actual rc=$actual"
    FAILURES=$((FAILURES + 1))
  fi
}

rc_of() {
  # runs a command, echoes its exit status; 128+N when killed by signal N
  local rc=0
  "$@" >>"$RUN_LOG" 2>&1 || rc=$?
  echo "$rc"
}

if [[ ! -x "$REPLAY" ]]; then
  msg="native replay binary not found: $REPLAY"
  if [[ "$REQUIRE_NATIVE" -eq 1 ]]; then
    fail "$msg"
  fi
  log "skip: $msg (build it with scripts/build_libfuzzer_gguf_native.sh)"
  exit 0
fi

mkdir -p "$OUT_DIR"
RUN_LOG="$OUT_DIR/oracle-runs.log"
: >"$RUN_LOG"

log "regenerating the malformed seeds"
bash "$PROJECT_ROOT/scripts/gen_gguf_malformed_seeds.sh" >>"$RUN_LOG" 2>&1 \
  || fail "seed generator failed; see $RUN_LOG"

log "replay selftest"
check "selftest" 0 "$(rc_of "$REPLAY" --selftest)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

# A header the parser reads cleanly and then turns down: version 99 is not a
# GGUF version it supports. 24 bytes = magic + version + n_tensors + n_kv.
python3 - "$tmp_dir/badversion.gguf" <<'PY'
import struct
import sys

with open(sys.argv[1], "wb") as fh:
    fh.write(b"GGUF" + struct.pack("<IQQ", 99, 0, 0))
PY
: >"$tmp_dir/empty.gguf"

log "rejected inputs must be exit 9, never a crash"
check "reject: version 99 header" 9 "$(rc_of "$REPLAY" "$tmp_dir/badversion.gguf")"
check "reject: empty file"        9 "$(rc_of "$REPLAY" "$tmp_dir/empty.gguf")"
check "reject: missing path"      9 "$(rc_of "$REPLAY" "$tmp_dir/does-not-exist.gguf")"
check "reject: no argument"       9 "$(rc_of "$REPLAY")"

log "assert-tripping inputs must really die (SIGABRT = 134)"
for name in align_wrongtype align_array2 emptykey; do
  poc="$MALFORMED_DIR/$name.gguf"
  [[ -f "$poc" ]] || fail "malformed seed missing: $poc"
  check "crash: $name" 134 "$(rc_of "$REPLAY" "$poc")"
done

log "well-formed seeds must pass at every depth"
shopt -s nullglob
seeds=("$SEED_DIR"/*.gguf)
shopt -u nullglob
[[ "${#seeds[@]}" -gt 0 ]] || fail "no seeds under $SEED_DIR"
for depth in metadata tensor-info full; do
  clean=0
  for seed in "${seeds[@]}"; do
    rc="$(GGUF_FUZZ_DEPTH="$depth" rc_of "$REPLAY" "$seed")"
    if [[ "$rc" != "0" ]]; then
      log "FAIL seed $depth $(basename "$seed") rc=$rc"
      FAILURES=$((FAILURES + 1))
    else
      clean=$((clean + 1))
    fi
  done
  log "OK   seeds depth=$depth clean=$clean/${#seeds[@]}"
done

# The contrast that justifies this harness: the same rejected file kills the
# probe we are replacing, because it dereferences the parser's NULL.
if [[ -x "$LEGACY_PROBE" ]]; then
  legacy_rc="$(GGML_NO_BACKTRACE=1 rc_of "$LEGACY_PROBE" --sha256 "$tmp_dir/badversion.gguf")"
  check "contrast: legacy probe segfaults on the rejected file" 139 "$legacy_rc"
else
  log "skip contrast: legacy probe not built at $LEGACY_PROBE"
fi

# prepare-target has to produce the same thing the harness links, and prove the
# parser is in it: libggml-base.a is produced even when gguf.cpp drops out of the
# target, so checking that the file exists checks nothing.
log "prepared target must build the archive the harness links, and prove gguf.cpp is in it"
grep -q 'ggml-base' "$PROJECT_ROOT/scripts/build_prepared_target.sh" \
  || fail "build_prepared_target.sh still builds llama-cli for gguf"
grep -q 'gguf_init_from_file_impl' "$PROJECT_ROOT/scripts/build_prepared_target.sh" \
  || fail "build_prepared_target.sh does not verify the gguf symbol"

if [[ "$FAILURES" -ne 0 ]]; then
  fail "$FAILURES check(s) failed; run log: $RUN_LOG"
fi
log "done: all checks passed (run log: $RUN_LOG)"
