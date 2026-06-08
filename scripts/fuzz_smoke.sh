#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: fuzz_smoke.sh [--target <onnx|gguf|safetensors>] [--backend <backend>|--backends <list>|--all] [--seconds <n>] [--id <campaign-id>] [--data-dir <dir>] [--seeds-dir <dir>] [--max-jobs <n>] [--workers <n>] [--timeout-sec <n>] [--dry-run]
EOF
}

WORKDIR="${WORKDIR:-$PWD}"
TOOL_BIN="${TOOL_BIN:-$WORKDIR/target/debug/tool}"
TARGET="onnx"
BACKENDS="local-harness"
SMOKE_SECONDS="10"
CAMPAIGN_ID=""
DATA_DIR="$WORKDIR/data"
SEEDS_DIR="$WORKDIR/seeds"
MAX_JOBS="1"
WORKERS="1"
TIMEOUT_SEC="10"
RESTART_LIMIT="0"
DRY_RUN="0"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target|-t) TARGET="${2:-}"; shift 2 ;;
    --backend|-b) BACKENDS="${2:-}"; shift 2 ;;
    --backends) BACKENDS="${2:-}"; shift 2 ;;
    --all) BACKENDS="local-harness,libfuzzer,aflpp"; shift ;;
    --seconds|-s) SMOKE_SECONDS="${2:-}"; shift 2 ;;
    --id|-i) CAMPAIGN_ID="${2:-}"; shift 2 ;;
    --data-dir) DATA_DIR="${2:-}"; shift 2 ;;
    --seeds-dir) SEEDS_DIR="${2:-}"; shift 2 ;;
    --max-jobs) MAX_JOBS="${2:-}"; shift 2 ;;
    --workers) WORKERS="${2:-}"; shift 2 ;;
    --timeout-sec) TIMEOUT_SEC="${2:-}"; shift 2 ;;
    --restart-limit) RESTART_LIMIT="${2:-}"; shift 2 ;;
    --dry-run) DRY_RUN="1"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[smoke] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

case "$TARGET" in
  onnx|gguf|safetensors) ;;
  *) echo "[smoke] unsupported target: $TARGET" >&2; exit 2 ;;
esac
if [[ ! "$SMOKE_SECONDS" =~ ^[0-9]+$ || "$SMOKE_SECONDS" -lt 1 ]]; then
  echo "[smoke] seconds must be an integer >= 1" >&2
  exit 2
fi
if [[ ! "$MAX_JOBS" =~ ^[0-9]+$ || "$MAX_JOBS" -lt 1 ]]; then
  echo "[smoke] max-jobs must be an integer >= 1" >&2
  exit 2
fi
if [[ ! "$WORKERS" =~ ^[0-9]+$ || "$WORKERS" -lt 1 ]]; then
  echo "[smoke] workers must be an integer >= 1" >&2
  exit 2
fi

safe_backends="$(printf '%s' "$BACKENDS" | tr ', ' '--' | tr -cd 'A-Za-z0-9._-')"
if [[ -z "$CAMPAIGN_ID" ]]; then
  CAMPAIGN_ID="smoke-${TARGET}-${safe_backends}-$(date +%Y%m%d-%H%M%S)"
fi

if [[ ! -x "$TOOL_BIN" ]]; then
  echo "[smoke] tool binary missing: $TOOL_BIN" >&2
  echo "[smoke] run: cargo build --offline" >&2
  exit 1
fi

cmd=(
  "$TOOL_BIN"
  --data-dir "$DATA_DIR"
  --seeds-dir "$SEEDS_DIR"
  campaign
  --mode parallel
  --target "$TARGET"
  --duration-seconds "$SMOKE_SECONDS"
  --campaign-id "$CAMPAIGN_ID"
  --backends "$BACKENDS"
  --workers "$WORKERS"
  --timeout-sec "$TIMEOUT_SEC"
  --restart-limit "$RESTART_LIMIT"
  --max-jobs "$MAX_JOBS"
)
if [[ "$DRY_RUN" == "1" ]]; then
  cmd+=(--dry-run)
fi

echo "[smoke] target=$TARGET backends=$BACKENDS seconds=$SMOKE_SECONDS id=$CAMPAIGN_ID"
printf '[smoke] command:'
printf ' %q' "${cmd[@]}"
printf '\n'
"${cmd[@]}"
