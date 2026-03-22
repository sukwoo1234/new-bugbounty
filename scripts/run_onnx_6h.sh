#!/usr/bin/env bash
set -euo pipefail

WORKDIR="${WORKDIR:-$HOME/bugbounty}"
HOOK_FILE="${HOOK_FILE:-$HOME/.config/bugbounty/discord_webhook}"
LOG_DIR="${LOG_DIR:-/tmp/longrun}"
LOG_FILE="${LOG_DIR}/run-onnx-loop-6h.log"
DONE_FILE="${LOG_DIR}/run-onnx-loop-6h.done"
DURATION_HOURS="${DURATION_HOURS:-6}"

mkdir -p "$LOG_DIR"

discord_notify() {
  local msg="$1"
  if [[ ! -f "$HOOK_FILE" ]]; then
    echo "[WARN] webhook file not found: $HOOK_FILE" | tee -a "$LOG_FILE"
    return 0
  fi
  local hook
  hook="$(cat "$HOOK_FILE")"
  if [[ -z "$hook" ]]; then
    echo "[WARN] webhook is empty" | tee -a "$LOG_FILE"
    return 0
  fi
  curl -sS -H "Content-Type: application/json" \
    -d "{\"content\":\"$msg\"}" \
    "$hook" >/dev/null || true
}

run_loop() {
  cd "$WORKDIR"
  local end_ts now_ts
  end_ts=$(( $(date +%s) + DURATION_HOURS * 3600 ))
  while true; do
    now_ts=$(date +%s)
    if [[ "$now_ts" -ge "$end_ts" ]]; then
      break
    fi
    ./target/debug/tool run --target onnx --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1
    sleep 2
  done
}

ts_start="$(date -Iseconds)"
echo "[START] onnx_${DURATION_HOURS}h ts=$ts_start host=$(hostname)" | tee -a "$LOG_FILE"
discord_notify "[START] onnx ${DURATION_HOURS}h ts=$ts_start host=$(hostname)"

set +e
run_loop >>"$LOG_FILE" 2>&1
ec=$?
set -e

ts_end="$(date -Iseconds)"
echo "[DONE] onnx_${DURATION_HOURS}h_finished ts=$ts_end exit=$ec host=$(hostname)" | tee -a "$LOG_FILE"
echo "$ts_end" > "$DONE_FILE"
discord_notify "[DONE] onnx ${DURATION_HOURS}h finished ts=$ts_end exit=$ec host=$(hostname)"
