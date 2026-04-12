#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: collect_longrun.sh --tag <TAG> [--out <dir>] [--tail <lines>]
EOF
}

TAG=""
OUT_DIR="/tmp"
TAIL_LINES="40"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) TAG="${2:-}"; shift 2 ;;
    --out) OUT_DIR="${2:-}"; shift 2 ;;
    --tail) TAIL_LINES="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[collect-longrun] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ -z "$TAG" ]]; then
  echo "[collect-longrun] --tag is required" >&2
  usage
  exit 2
fi

LOG_FILE="data/longrun/run-${TAG}.log"
EXIT_FILE="data/longrun/run-${TAG}.exit"
METRICS_FILE="data/metrics/latest.json"

if [[ ! -f "$LOG_FILE" ]]; then
  echo "[collect-longrun] log file not found: $LOG_FILE" >&2
  exit 2
fi
if [[ ! -f "$EXIT_FILE" ]]; then
  echo "[collect-longrun] exit file not found: $EXIT_FILE" >&2
  exit 2
fi
if [[ ! -f "$METRICS_FILE" ]]; then
  echo "[collect-longrun] metrics file not found: $METRICS_FILE" >&2
  exit 2
fi

mkdir -p "$OUT_DIR"
SNAPSHOT_PATH="${OUT_DIR%/}/metrics-${TAG}.json"
cp "$METRICS_FILE" "$SNAPSHOT_PATH"

echo "[collect-longrun] tail -n ${TAIL_LINES} ${LOG_FILE}"
tail -n "$TAIL_LINES" "$LOG_FILE"
echo "[collect-longrun] exit file: ${EXIT_FILE}"
cat "$EXIT_FILE"

EXIT_CODE="$(sed -n 's/^exit_code=//p' "$EXIT_FILE" | head -n 1)"
RUNS="$(sed -n 's/^runs=//p' "$EXIT_FILE" | head -n 1)"
FAILURES="$(sed -n 's/^failures=//p' "$EXIT_FILE" | head -n 1)"
echo "[collect-longrun] summary exit_code=${EXIT_CODE:-unknown} runs=${RUNS:-unknown} failures=${FAILURES:-unknown} metrics_snapshot=${SNAPSHOT_PATH}"
