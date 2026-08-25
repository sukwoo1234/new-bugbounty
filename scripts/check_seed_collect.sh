#!/usr/bin/env bash
# A12: archive members that share a basename must not overwrite each other on the
# way into the raw seed directory - a dropped member is a seed the campaign never
# sees. Uses only a temp tree; no network, no tool binary.
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

log() {
  echo "[seed-collect-check] $*"
}

fail() {
  echo "[seed-collect-check] fail: $*" >&2
  exit 1
}

# shellcheck source=lib/seed_collect.sh
. "$PROJECT_ROOT/scripts/lib/seed_collect.sh"

stage="$WORK/stage"
raw="$WORK/raw"
mkdir -p "$stage/dirA" "$stage/dirB" "$stage/nested/deep"
printf 'AAAA' > "$stage/dirA/model.onnx"
printf 'BBBB' > "$stage/dirB/model.onnx"
printf 'CCCC' > "$stage/nested/deep/model.onnx"
printf 'skip' > "$stage/dirA/notes.txt"

collect_seed_files "$stage" "$raw" "onnx"

count="$(find "$raw" -maxdepth 1 -type f -name '*.onnx' | wc -l | tr -d ' ')"
[[ "$count" -eq 3 ]] || fail "expected 3 collected files, got $count"
log "ok: three same-named members all survived the flatten"

for want in AAAA BBBB CCCC; do
  found=0
  for f in "$raw"/*.onnx; do
    if [[ "$(cat "$f")" == "$want" ]]; then
      found=1
      break
    fi
  done
  [[ "$found" -eq 1 ]] || fail "member with body '$want' was dropped"
done
log "ok: every member's content is present exactly once"

for f in "$raw"/*; do
  case "$f" in
    *.onnx) ;;
    *) fail "collected name does not end in .onnx: $f" ;;
  esac
done
log "ok: every collected name still ends in .onnx"

[[ ! -e "$raw/notes.txt" ]] || fail "a non-matching extension was collected"
log "ok: non-matching extensions are left behind"

log "all checks passed"
