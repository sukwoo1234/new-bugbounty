#!/usr/bin/env bash
set -euo pipefail

WORKDIR="${WORKDIR:-$PWD}"
TARGET="${TARGET:-onnx}"
BACKEND="${BACKEND:-local-harness}"
CORPUS_DIR="${CORPUS_DIR:-seeds/${TARGET}}"
WORKERS="${WORKERS:-2}"
TIMEOUT_SEC="${TIMEOUT_SEC:-30}"
RESTART_LIMIT="${RESTART_LIMIT:-1}"
MAX_JOBS="${MAX_JOBS:-}"
LOOP_SLEEP_SEC="${LOOP_SLEEP_SEC:-2}"

DURATION_HOURS="${DURATION_HOURS:-6}"
DURATION_SECONDS="${DURATION_SECONDS:-}"
if [[ -n "${DURATION_SECONDS}" ]]; then
  DURATION_SECS="${DURATION_SECONDS}"
  DURATION_LABEL="${DURATION_SECONDS}s"
else
  DURATION_SECS=$((DURATION_HOURS * 3600))
  DURATION_LABEL="${DURATION_HOURS}h"
fi

LOG_DIR="${LOG_DIR:-$WORKDIR/data/longrun}"
mkdir -p "$LOG_DIR"

TAG="${TAG:-${TARGET}_${BACKEND}_${DURATION_LABEL}}"
LOG_FILE="${LOG_FILE:-$LOG_DIR/run-${TAG}.log}"
DONE_FILE="${DONE_FILE:-$LOG_DIR/run-${TAG}.done}"
EXIT_FILE="${EXIT_FILE:-$LOG_DIR/run-${TAG}.exit}"

run_once() {
  local -a cmd
  cmd=(
    "$WORKDIR/target/debug/tool" run
    --target "$TARGET"
    --backend "$BACKEND"
    --corpus-dir "$CORPUS_DIR"
    --workers "$WORKERS"
    --timeout-sec "$TIMEOUT_SEC"
    --restart-limit "$RESTART_LIMIT"
  )
  if [[ -n "$MAX_JOBS" ]]; then
    cmd+=(--max-jobs "$MAX_JOBS")
  fi

  "${cmd[@]}"
}

ts_start="$(date -Iseconds)"
echo "[START] ${TAG} ts=${ts_start} host=$(hostname)" | tee -a "$LOG_FILE"

end_ts=$(( $(date +%s) + DURATION_SECS ))
runs=0
failures=0
last_ec=0

cd "$WORKDIR"
while true; do
  now_ts=$(date +%s)
  if [[ "$now_ts" -ge "$end_ts" ]]; then
    break
  fi

  set +e
  run_once >>"$LOG_FILE" 2>&1
  ec=$?
  set -e

  runs=$((runs + 1))
  last_ec="$ec"
  if [[ "$ec" -ne 0 ]]; then
    failures=$((failures + 1))
  fi

  sleep "$LOOP_SLEEP_SEC"
done

ts_end="$(date -Iseconds)"
echo "[DONE] ${TAG} ts=${ts_end} exit=${last_ec} runs=${runs} failures=${failures} host=$(hostname)" | tee -a "$LOG_FILE"
echo "${ts_end}" > "$DONE_FILE"
cat > "$EXIT_FILE" <<EOF
timestamp=${ts_end}
exit_code=${last_ec}
runs=${runs}
failures=${failures}
tag=${TAG}
EOF

# terminal bell for local attention
printf '\a'
