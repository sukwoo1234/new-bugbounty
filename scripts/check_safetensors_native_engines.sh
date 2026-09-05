#!/usr/bin/env bash
# Prove the safetensors libFuzzer arm actually runs the real parser, not the tool
# wrapper. Mirrors check_gguf_native_engines.sh, with one honest difference: the
# safetensors crate is memory-safe and audited, so there is NO crashing PoC to assert
# (deserialize holds). Instead this check requires: the in-process libFuzzer target runs
# a real corpus clean, and the replay's accept/reject boundary is the crate's.
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
FUZZER="${FUZZER:-$PROJECT_ROOT/harnesses/libfuzzer/safetensors_loader_fuzzer}"
REPLAY="${REPLAY:-$PROJECT_ROOT/harnesses/libfuzzer/safetensors_loader_replay}"
AFLPP_REPLAY="${AFLPP_REPLAY:-$PROJECT_ROOT/harnesses/aflpp/safetensors_loader_replay}"
SEED_DIR="${SEED_DIR:-$PROJECT_ROOT/seeds/safetensors}"
MAL_DIR="${MAL_DIR:-$PROJECT_ROOT/seeds/safetensors-malformed}"
RUNS="${RUNS:-20000}"
AFLPP_CHECK_DIR="${AFLPP_CHECK_DIR:-$PROJECT_ROOT/data/native-engine-checks}"
REQUIRE_AFLPP=0

log() { echo "[st-engines] $*"; }
fail() { echo "[st-engines] fail: $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --require-aflpp) REQUIRE_AFLPP=1; shift ;;
    -h|--help) echo "usage: check_safetensors_native_engines.sh [--require-aflpp]"; exit 0 ;;
    *) echo "[st-engines] unknown arg: $1" >&2; exit 2 ;;
  esac
done

# build the harness if missing
if [[ ! -x "$FUZZER" || ! -x "$REPLAY" ]]; then
  log "harness missing; building"
  bash "$PROJECT_ROOT/scripts/build_libfuzzer_safetensors_native.sh" >/dev/null
fi
[[ -x "$FUZZER" ]] || fail "libFuzzer target not built: $FUZZER"
[[ -x "$REPLAY" ]] || fail "replay not built: $REPLAY"

# 1. the in-process libFuzzer target runs a real corpus clean (no crash artifact). This
#    is where a crash WOULD surface if deserialize had one; a clean run corroborates the
#    crate's 'safe' claim rather than a tooling failure.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$SEED_DIR"/*.safetensors "$WORK"/ 2>/dev/null || true
[[ -n "$(ls -A "$WORK" 2>/dev/null)" ]] || fail "no seeds to run under $SEED_DIR"
set +e
"$FUZZER" -runs="$RUNS" -max_len=1048576 -artifact_prefix="$WORK/" "$WORK" >/dev/null 2>&1
rc=$?
set -e
crashes="$(find "$WORK" -maxdepth 1 -name 'crash-*' -o -name 'oom-*' -o -name 'timeout-*' 2>/dev/null | wc -l)"
if [[ "$crashes" -ne 0 ]]; then
  log "NOTE: the libFuzzer target produced $crashes crash artifact(s) under $WORK - triage them"
  fail "unexpected crash artifacts (deserialize is expected to hold)"
fi
[[ $rc -eq 0 ]] || fail "libFuzzer target exited $rc on a clean corpus"
log "libFuzzer target ran $RUNS runs clean (held, as expected for the audited crate)"

# 2. the replay's accept/reject boundary is the crate's: good -> 0, malformed -> 9
if [[ ! -d "$MAL_DIR" ]] || [[ -z "$(ls -A "$MAL_DIR" 2>/dev/null)" ]]; then
  bash "$PROJECT_ROOT/scripts/gen_safetensors_malformed_seeds.sh" >/dev/null
fi
GOOD="$(find "$SEED_DIR" -name '*.safetensors' | head -1)"
[[ -n "$GOOD" ]] || fail "no valid safetensors seed under $SEED_DIR"
"$REPLAY" "$GOOD" >/dev/null 2>&1 || fail "replay rejected a valid seed"
POC="$(find "$MAL_DIR" -name '*.safetensors' | head -1)"
set +e; "$REPLAY" "$POC" >/dev/null 2>&1; rc=$?; set -e
[[ $rc -eq 9 ]] || fail "replay gave exit $rc on a malformed PoC, expected 9"
log "replay accept/reject boundary is the crate's (good->0, malformed->9)"

# 3. AFL++ arm. Rust is built through cargo-afl; a host with a standalone afl-showmap
# can validate an already-built replay as well. Only the explicit --require-aflpp mode
# turns missing tooling into a hard failure.
. "$PROJECT_ROOT/scripts/lib/engine_mode.sh"

AFLPP_SHOWMAP_MODE=""
if command -v cargo-afl >/dev/null 2>&1; then
  AFLPP_SHOWMAP_MODE="cargo-afl"
elif command -v afl-showmap >/dev/null 2>&1; then
  AFLPP_SHOWMAP_MODE="standalone"
fi

run_aflpp_showmap() {
  if [[ "$AFLPP_SHOWMAP_MODE" == "cargo-afl" ]]; then
    local toolchain="${CARGO_AFL_TOOLCHAIN:-nightly}"
    if [[ -n "$toolchain" ]]; then
      cargo "+$toolchain" afl showmap "$@"
    else
      cargo afl showmap "$@"
    fi
  else
    afl-showmap "$@"
  fi
}

if [[ -n "$AFLPP_SHOWMAP_MODE" ]]; then
  mkdir -p "$AFLPP_CHECK_DIR"
  if [[ "$AFLPP_SHOWMAP_MODE" == "cargo-afl" ]]; then
    log "build AFL++ safetensors replay with cargo-afl"
    OUT="$AFLPP_REPLAY" bash "$PROJECT_ROOT/scripts/build_aflpp_safetensors_native.sh" \
      >"$AFLPP_CHECK_DIR/safetensors-aflpp-build.log" 2>&1 \
      || fail "AFL++ Rust build failed; see $AFLPP_CHECK_DIR/safetensors-aflpp-build.log"
  fi

  [[ -x "$AFLPP_REPLAY" ]] \
    || fail "AFL++ replay not produced: $AFLPP_REPLAY"
  scope="$(instrumentation_scope "$AFLPP_REPLAY")"
  [[ "$scope" == "library" ]] \
    || fail "AFL++ safetensors replay scope is '$scope', expected library"
  log "AFL++ replay scope=library"

  AFL_MAP="$AFLPP_CHECK_DIR/safetensors-aflpp-showmap.txt"
  rm -f "$AFL_MAP"
  showmap_rc=0
  run_aflpp_showmap -q -o "$AFL_MAP" -- "$AFLPP_REPLAY" "$GOOD" \
    >"$AFLPP_CHECK_DIR/safetensors-aflpp-showmap.log" 2>&1 \
    || showmap_rc=$?
  [[ -f "$AFL_MAP" ]] \
    || fail "afl-showmap wrote no map (rc=$showmap_rc); see $AFLPP_CHECK_DIR/safetensors-aflpp-showmap.log"
  tuples="$(wc -l < "$AFL_MAP" | tr -d ' ')"
  [[ "$tuples" -gt 0 ]] \
    || fail "afl-showmap produced zero tuples (rc=$showmap_rc); see $AFLPP_CHECK_DIR/safetensors-aflpp-showmap.log"
  log "afl-showmap tuples=$tuples (rc=$showmap_rc, mode=$AFLPP_SHOWMAP_MODE)"
elif [[ "$REQUIRE_AFLPP" -eq 1 ]]; then
  fail "AFL++ tooling missing: install cargo-afl or provide afl-showmap"
else
  log "AFL++ tooling missing (cargo-afl/afl-showmap); skipping (pass --require-aflpp to enforce)"
fi

log "ok"
