#!/usr/bin/env bash
set -euo pipefail

WORKDIR="${WORKDIR:-$PWD}"
BIND="${BIND:-127.0.0.1:8787}"
BASE_URL="http://${BIND}"
LOG_DIR="${LOG_DIR:-$WORKDIR/data/ui-check}"
mkdir -p "$LOG_DIR"

SERVER_LOG="${LOG_DIR}/ui-serve.log"
CHECK_LOG="${LOG_DIR}/ui-routes-check.log"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

cd "$WORKDIR"

cargo run --offline -- ui-serve --bind "$BIND" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

wait_for_server() {
  local max_tries="${1:-25}"
  local i=1
  while [[ "$i" -le "$max_tries" ]]; do
    if curl -fsS "${BASE_URL}/healthz" >/dev/null 2>&1; then
      return 0
    fi
    if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
      echo "[FAIL] ui-serve exited before healthz became ready" | tee -a "$CHECK_LOG"
      echo "[FAIL] server_log: $SERVER_LOG" | tee -a "$CHECK_LOG"
      tail -n 30 "$SERVER_LOG" | sed 's/^/[server] /' | tee -a "$CHECK_LOG"
      return 1
    fi
    sleep 0.2
    i=$((i + 1))
  done
  echo "[FAIL] ui-serve healthz not ready in time" | tee -a "$CHECK_LOG"
  echo "[FAIL] server_log: $SERVER_LOG" | tee -a "$CHECK_LOG"
  tail -n 30 "$SERVER_LOG" | sed 's/^/[server] /' | tee -a "$CHECK_LOG"
  return 1
}

check_url() {
  local url="$1"
  if curl -fsS "$url" >/dev/null; then
    echo "[OK] $url" | tee -a "$CHECK_LOG"
  else
    echo "[FAIL] $url" | tee -a "$CHECK_LOG"
    return 1
  fi
}

: >"$CHECK_LOG"
wait_for_server
check_url "${BASE_URL}/healthz"
check_url "${BASE_URL}/dashboard.html"
check_url "${BASE_URL}/dashboard.json"
check_url "${BASE_URL}/assets/dashboard.css"
check_url "${BASE_URL}/control/status"
check_url "${BASE_URL}/replay/status"
check_url "${BASE_URL}/target/status"
check_url "${BASE_URL}/target/build/status"

# A4: a client that opens the socket and sends nothing used to block the single-threaded
# accept loop, denying every other request until it went away.
check_stalled_client_does_not_block() {
  local host="${BIND%:*}"
  local port="${BIND##*:}"
  if ! exec 9<>"/dev/tcp/${host}/${port}" 2>/dev/null; then
    echo "[FAIL] could not open a stalled connection to ${BIND}" | tee -a "$CHECK_LOG"
    return 1
  fi
  local rc=0
  if curl -fsS --max-time 5 "${BASE_URL}/healthz" >/dev/null 2>&1; then
    echo "[OK] stalled client does not block the dashboard" | tee -a "$CHECK_LOG"
  else
    echo "[FAIL] a client that sends nothing blocked the dashboard" | tee -a "$CHECK_LOG"
    rc=1
  fi
  exec 9<&-
  exec 9>&-
  return "$rc"
}

check_stalled_client_does_not_block

dash_html="$(curl -fsS "${BASE_URL}/dashboard.html")"
run_path="$(printf '%s' "$dash_html" | sed -n 's/.*href="\(\/*run\/[^"]*\)".*/\1/p' | head -n 1)"
triage_path="$(printf '%s' "$dash_html" | sed -n 's/.*href="\(\/*triage\/[^"]*\)".*/\1/p' | head -n 1)"
report_path="$(printf '%s' "$dash_html" | sed -n 's/.*href="\(\/*report\/[^"]*\)".*/\1/p' | head -n 1)"
coverage_path="$(printf '%s' "$dash_html" | sed -n 's/.*href="\(\/*coverage\/[^"]*\)".*/\1/p' | head -n 1)"

[[ -n "$run_path" ]] && check_url "${BASE_URL}${run_path}"
[[ -n "$triage_path" ]] && check_url "${BASE_URL}${triage_path}"
[[ -n "$report_path" ]] && check_url "${BASE_URL}${report_path}"
[[ -n "$coverage_path" ]] && check_url "${BASE_URL}${coverage_path}"

echo "[ui-check] done"
echo "server_log: $SERVER_LOG"
echo "check_log: $CHECK_LOG"
