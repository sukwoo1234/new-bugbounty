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

# Review follow-up: the extraction moved the walk into a process substitution,
# whose exit status bash never collects - so a directory the archive restored
# without read permission produced a silently partial corpus and exit 0. That is
# the same silent seed loss this check exists to prevent.
locked_stage="$WORK/locked-stage"
locked_raw="$WORK/locked-raw"
mkdir -p "$locked_stage/ok" "$locked_stage/denied"
printf 'OK' > "$locked_stage/ok/a.onnx"
printf 'DENIED' > "$locked_stage/denied/b.onnx"
chmod 000 "$locked_stage/denied"

if find "$locked_stage" -type f -name '*.onnx' >/dev/null 2>&1; then
  # Running as root, where the mode is not enforced.
  log "skip: unreadable-directory case (running as root)"
else
  set +e
  collect_seed_files "$locked_stage" "$locked_raw" "onnx"
  rc=$?
  set -e
  chmod 755 "$locked_stage/denied"
  [[ "$rc" -ne 0 ]] || fail "a directory that could not be walked was reported as success"
  log "ok: an unreadable subdirectory fails the collection instead of truncating it"
fi

log "all checks passed"
