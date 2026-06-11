#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
TOOL_BIN="${TOOL_BIN:-$PROJECT_ROOT/target/debug/tool}"
SEED_DIR="${SEED_DIR:-$PROJECT_ROOT/seeds/onnx}"
OUT_DIR="${OUT_DIR:-$PROJECT_ROOT/data/corpus/onnx-aggressive}"
COUNT="${COUNT:-1000}"
SEED="${SEED:-1337}"
OPERATORS="${OPERATORS:-aggressive}"

usage() {
  cat <<'EOF'
usage: build_onnx_aggressive_corpus.sh

Builds an ONNX crash-hunting corpus with explicit operator selection.

Environment:
  PROJECT_ROOT=/home/ssw/bugbounty
  TOOL_BIN=/home/ssw/bugbounty/target/debug/tool
  SEED_DIR=/home/ssw/bugbounty/seeds/onnx
  OUT_DIR=/home/ssw/bugbounty/data/corpus/onnx-aggressive
  COUNT=1000
  SEED=1337
  OPERATORS=aggressive

OPERATORS is comma-separated and is passed as repeatable --operator flags.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    *) echo "[onnx-aggressive-corpus] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

log() {
  echo "[onnx-aggressive-corpus] $*"
}

fail() {
  echo "[onnx-aggressive-corpus] fail: $*" >&2
  exit 1
}

[[ -d "$SEED_DIR" ]] || fail "seed dir not found: $SEED_DIR"

if [[ ! -x "$TOOL_BIN" ]]; then
  log "$TOOL_BIN missing; building debug tool"
  cargo build --offline >/tmp/onnx-aggressive-corpus-cargo-build.log
fi

operator_args=()
IFS=',' read -r -a requested_operators <<< "$OPERATORS"
for raw_operator in "${requested_operators[@]}"; do
  operator="${raw_operator//[[:space:]]/}"
  if [[ -n "$operator" ]]; then
    operator_args+=(--operator "$operator")
  fi
done
[[ "${#operator_args[@]}" -gt 0 ]] || fail "OPERATORS produced no --operator args"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

log "mutate start: seed_dir=$SEED_DIR out_dir=$OUT_DIR count=$COUNT seed=$SEED operators=$OPERATORS"
"$TOOL_BIN" mutate \
  --target onnx \
  --input-dir "$SEED_DIR" \
  --out-dir "$OUT_DIR" \
  --count "$COUNT" \
  --seed "$SEED" \
  "${operator_args[@]}" \
  >"$OUT_DIR/mutate.log" 2>&1

manifest="$OUT_DIR/manifest.json"
[[ -f "$manifest" ]] || fail "manifest not found: $manifest"

python3 - "$manifest" "$OPERATORS" <<'PY'
import json
import sys

manifest_path, expected_csv = sys.argv[1:3]
with open(manifest_path, encoding="utf-8") as file:
    manifest = json.load(file)

generated = manifest.get("generated", 0)
if not isinstance(generated, int) or generated <= 0:
    raise SystemExit(f"generated is not positive: {generated!r}")

requested = manifest.get("operators_requested", [])
expected = [item.strip() for item in expected_csv.split(",") if item.strip()]
missing = [item for item in expected if item not in requested]
if missing:
    raise SystemExit(f"missing operators in manifest: {missing}; requested={requested}")
PY

log "done: out_dir=$OUT_DIR manifest=$manifest"
