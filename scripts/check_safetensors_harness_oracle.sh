#!/usr/bin/env bash
# Prove the safetensors native replay honors the 0/9/10 exit contract on real files.
# Mirrors check_gguf_harness_oracle.sh, but the oracle here is the Rust crate: a valid
# file is accepted (0), a malformed file is a clean rejection (9, NOT a crash), and a
# missing binary is a hard failure (never a silent pass).
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
REPLAY="${REPLAY:-$PROJECT_ROOT/harnesses/libfuzzer/safetensors_loader_replay}"
SEED_DIR="${SEED_DIR:-$PROJECT_ROOT/seeds/safetensors}"
MAL_DIR="${MAL_DIR:-$PROJECT_ROOT/seeds/safetensors-malformed}"

log() { echo "[st-oracle] $*"; }
fail() { echo "[st-oracle] fail: $*" >&2; exit 1; }

[[ -x "$REPLAY" ]] || fail "replay not executable: $REPLAY (build it with scripts/build_libfuzzer_safetensors_native.sh)"

# selftest prints the exit-code contract
"$REPLAY" --selftest | grep -q "exit_codes: ok=0 rejected=9 unavailable=10" \
  || fail "replay --selftest did not print the expected contract"
log "selftest contract ok"

# a good seed is accepted (exit 0)
GOOD="$SEED_DIR/safe_00.safetensors"
[[ -f "$GOOD" ]] || GOOD="$(find "$SEED_DIR" -name '*.safetensors' | head -1)"
[[ -n "$GOOD" ]] || fail "no valid seed found under $SEED_DIR"
"$REPLAY" "$GOOD" >/dev/null 2>&1
rc=$?
[[ $rc -eq 0 ]] || fail "valid seed $GOOD gave exit $rc, expected 0"
log "valid seed accepted (exit 0): $(basename "$GOOD")"

# malformed seeds are cleanly rejected (exit 9), never a crash. Generate them if absent.
if [[ ! -d "$MAL_DIR" ]] || [[ -z "$(ls -A "$MAL_DIR" 2>/dev/null)" ]]; then
  log "malformed seeds missing; generating"
  bash "$PROJECT_ROOT/scripts/gen_safetensors_malformed_seeds.sh" >/dev/null
fi
checked=0
for m in "$MAL_DIR"/*.safetensors; do
  [[ -f "$m" ]] || continue
  set +e
  "$REPLAY" "$m" >/dev/null 2>&1
  rc=$?
  set -e
  # 9 = clean reject. A signal death (>128) would be a real crash and must fail here.
  if [[ $rc -ne 9 ]]; then
    fail "malformed seed $(basename "$m") gave exit $rc, expected 9 (clean reject)"
  fi
  checked=$((checked + 1))
done
[[ $checked -gt 0 ]] || fail "no malformed seeds checked"
log "all $checked malformed seeds cleanly rejected (exit 9, no crash)"
log "ok"
