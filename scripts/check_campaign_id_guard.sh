#!/usr/bin/env bash
# A30: re-running a campaign with an id that already exists truncated status.tsv
# and overwrote manifest.json, so the record of the first campaign - the one whose
# results a paper cites - was destroyed by a mistyped resume. Fixture-only: no
# fuzzer, no ONNX Runtime, no network.
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
SCRIPT="$PROJECT_ROOT/scripts/run_onnx_abc_week.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

log() {
  echo "[campaign-guard-check] $*"
}

fail() {
  echo "[campaign-guard-check] fail: $*" >&2
  exit 1
}

# A workdir the script can cd into, with just enough of the tree to get past the
# ONNX Runtime lookup and the preflight binary check.
mkdir -p "$WORK/seeds/onnx" "$WORK/target/release" "$WORK/data/campaigns"
printf 'seed' > "$WORK/seeds/onnx/seed.onnx"
printf '#!/usr/bin/env bash\nexit 0\n' > "$WORK/target/release/tool"
chmod +x "$WORK/target/release/tool"
ORT_FIXTURE="$WORK/ort/build/Linux/Release"
mkdir -p "$ORT_FIXTURE"
printf 'so' > "$ORT_FIXTURE/libonnxruntime.so"

# The campaign starts a background host monitor that inherits stdout, so command
# substitution would block past the timeout. Capture to a file instead.
run_script() {
  local campaign_id="$1"
  shift
  : > "$WORK/out.log"
  ( cd "$WORK" && \
    WORKDIR="$WORK" ORT_SRC="$WORK/ort" \
    timeout -k 2 5 bash "$SCRIPT" \
      --campaign-id "$campaign_id" \
      --skip-preflight \
      --block-seconds 1 \
      --days 1 \
      "$@" ) > "$WORK/out.log" 2>&1 < /dev/null
}

# --- case 1: an existing campaign id is refused and its ledger survives --------
existing="$WORK/data/campaigns/c1"
mkdir -p "$existing"
printf 'sentinel\n' > "$existing/status.tsv"
printf '{"campaign_id":"c1"}\n' > "$existing/manifest.json"

set +e
run_script c1
rc=$?
out="$(cat "$WORK/out.log")"
set -e

[[ "$rc" -ne 0 ]] || fail "a second run with an existing --campaign-id must not succeed (exit $rc)"
grep -q 'campaign already exists' <<<"$out" || fail "expected a 'campaign already exists' message, got: $out"
[[ "$(cat "$existing/status.tsv")" == "sentinel" ]] || fail "status.tsv of the existing campaign was overwritten"
grep -q '"campaign_id":"c1"' "$existing/manifest.json" || fail "manifest.json of the existing campaign was overwritten"
log "ok: an existing campaign id is refused and its ledger is intact"

# --- case 2: a fresh id still gets past the guard -----------------------------
set +e
run_script c2
rc=$?
out="$(cat "$WORK/out.log")"
set -e

# The script runs a real campaign after the guard, so the timeout killing it (124)
# is the expected outcome here; what matters is that it got past the guard.
[[ "$rc" -eq 124 || "$rc" -eq 0 ]] || {
  grep -q 'campaign already exists' <<<"$out" && fail "a fresh campaign id was refused"
  log "note: run ended with exit $rc"
}
grep -q 'starting campaign=c2' <<<"$out" || fail "a fresh campaign id did not get past the guard, got: $out"
log "ok: a fresh campaign id still starts"

# --- case 3: --dry-run stays unguarded ----------------------------------------
set +e
run_script c1 --dry-run
rc=$?
out="$(cat "$WORK/out.log")"
set -e
[[ "$rc" -eq 0 ]] || fail "--dry-run must stay usable for an existing id (exit $rc)"
grep -q 'dry-run campaign_id=c1' <<<"$out" || fail "--dry-run did not print its plan, got: $out"
log "ok: --dry-run is not blocked by the guard"

log "all checks passed"
