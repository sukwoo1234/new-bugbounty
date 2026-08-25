#!/usr/bin/env bash
# A31: the 2026-04-18 recovery re-triage re-derives each summary's verdict from its
# attempts. A summary with an empty attempts[] satisfied "every attempt crashed"
# vacuously and was stamped verdict=reproduced - a crash claim with no evidence
# behind it. Fixture-only: temp data dir, jq, no tool binary.
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
SCRIPT="$PROJECT_ROOT/scripts/retriage_from_raw.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

log() {
  echo "[retriage-check] $*"
}

fail() {
  echo "[retriage-check] fail: $*" >&2
  exit 1
}

TRIAGE_ROOT="$WORK/data/triage"
mkdir -p "$TRIAGE_ROOT"

# write_summary <id> <attempts-json>
write_summary() {
  mkdir -p "$TRIAGE_ROOT/triage-$1"
  cat > "$TRIAGE_ROOT/triage-$1/summary.json" <<SUMEOF
{
  "triage_id": "triage-$1",
  "verdict": "reproduced",
  "attempts": $2
}
SUMEOF
}

verdict_of() {
  jq -r '.verdict' "$TRIAGE_ROOT/triage-$1/summary-v2.json"
}

attempt() {
  # attempt <result> <signature-json>
  printf '{"result": "%s", "signature_top3": %s}' "$1" "$2"
}

SIG_A='["frame_one","frame_two"]'
SIG_B='["other_one","other_two"]'

write_summary empty '[]'
write_summary all-crashed "[$(attempt failed "$SIG_A"), $(attempt failed "$SIG_A")]"
write_summary one-crashed "[$(attempt failed "$SIG_A"), $(attempt success "$SIG_A")]"
write_summary mismatched "[$(attempt failed "$SIG_A"), $(attempt failed "$SIG_B")]"

bash "$SCRIPT" --data-dir "$WORK/data" > "$WORK/out.log" 2>&1 \
  || fail "retriage exited non-zero: $(cat "$WORK/out.log")"

# --- case 1: an empty attempt list is not a reproduced crash ------------------
if [[ -e "$TRIAGE_ROOT/triage-empty/summary-v2.json" ]]; then
  got="$(verdict_of empty)"
  [[ "$got" != "reproduced" ]] || fail "an empty attempts[] was stamped verdict=reproduced"
  log "ok: an empty attempt list was re-stamped '$got', not reproduced"
else
  grep -q 'no attempts' "$WORK/out.log" || fail "an empty attempts[] was skipped without saying why"
  log "ok: an empty attempt list is skipped with a reason instead of stamped"
fi

# --- case 2: every attempt crashed with one signature -> reproduced -----------
[[ "$(verdict_of all-crashed)" == "reproduced" ]] \
  || fail "a genuinely reproduced crash was re-stamped $(verdict_of all-crashed)"
log "ok: every attempt crashing with one signature is still reproduced"

# --- case 3: a lone crash with a matching signature -> flaky -----------------
[[ "$(verdict_of one-crashed)" == "flaky" ]] \
  || fail "a lone crash was re-stamped $(verdict_of one-crashed)"
log "ok: a lone crash with a matching signature is flaky"

# --- case 4: signatures that disagree -> flaky_stack_mismatch ----------------
# Note: this script compares signature_top3 across ALL attempts, while
# src/triage.rs compares normalized_frame_hash across the crashed ones only. The
# case is here so that divergence is pinned by a test rather than left implicit.
[[ "$(verdict_of mismatched)" == "flaky_stack_mismatch" ]] \
  || fail "disagreeing signatures were re-stamped $(verdict_of mismatched)"
log "ok: disagreeing signatures are flaky_stack_mismatch"

# The evidence-less summary must be reported on its own line, not folded into the
# "already migrated, no work needed" tally.
grep -q 'skipped (no attempts, verdict left unverified): 1' "$WORK/out.log" \
  || fail "an evidence-less summary was not reported separately: $(cat "$WORK/out.log")"
log "ok: an evidence-less summary is counted and reported on its own"

log "all checks passed"
