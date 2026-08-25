#!/usr/bin/env bash
# Shared seed collection: flatten a staged archive tree into one raw directory.
#
# Sourced by scripts/seed_fetch.sh and exercised by scripts/check_seed_collect.sh.

# A12: a plain `cp "$f" "$raw_dir/"` keeps only the basename, so two archive
# members named model.onnx in different directories silently overwrote each other
# and the campaign never saw the dropped one. The name carries the member's path.
#
# collect_seed_files <stage_dir> <raw_dir> <ext>
collect_seed_files() {
  local stage_dir="$1"
  local raw_dir="$2"
  local ext="$3"
  mkdir -p "$raw_dir"

  # The walk's exit status has to be checked. A `find | while` pipeline propagated
  # it under `set -o pipefail`; a `while ... < <(find)` process substitution does
  # not, so a subdirectory the archive restored unreadable would yield a silently
  # partial corpus and a success return - the same silent seed loss this function
  # exists to prevent.
  local listing
  listing="$(mktemp)" || return 1
  if ! find "$stage_dir" -type f -name "*.${ext}" -print0 > "$listing"; then
    rm -f "$listing"
    echo "[seed-collect] failed to walk '$stage_dir'" >&2
    return 1
  fi

  local f rel flat dest
  while IFS= read -r -d '' f; do
    rel="${f#"$stage_dir"/}"
    flat="${rel//\//_}"
    # Deeply nested members can outgrow the filesystem's name limit.
    if [[ "${#flat}" -gt 200 ]]; then
      flat="$(seed_collect_digest "$rel")_$(basename -- "$f")"
    fi
    dest="$raw_dir/$flat"
    # Two different paths can still flatten to one name (a/b_c vs a_b/c).
    if [[ -e "$dest" ]]; then
      dest="$raw_dir/$(seed_collect_digest "$rel")_$(basename -- "$f")"
    fi
    cp "$f" "$dest"
  done < "$listing"
  rm -f "$listing"
}

seed_collect_digest() {
  printf '%s' "$1" | sha256sum | cut -c1-16
}
