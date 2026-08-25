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
  if curl -fsS --max-time 10 "$url" >/dev/null; then
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

# Asserts the exact HTTP status of a request that must be refused before anything is spawned.
check_status() {
  local expected="$1"
  local method="$2"
  local url="$3"
  local label="$4"
  local got
  got="$(curl -s -o /dev/null -w '%{http_code}' -X "$method" "$url" || true)"
  if [[ "$got" == "$expected" ]]; then
    echo "[OK] $label -> $got" | tee -a "$CHECK_LOG"
  else
    echo "[FAIL] $label -> expected $expected, got $got" | tee -a "$CHECK_LOG"
    return 1
  fi
}

# A2: a newline in a query value used to inject an extra `pid=` line into the state file, which
# /target/stop then handed to kill. A16: `version` reached build_prepared_target.sh's rm -rf.
check_rejected_inputs() {
  check_status 400 POST "${BASE_URL}/target/prepare?target=onnx&version=x%0Apid%3D1234" \
    "A2 newline in version"
  check_status 400 POST "${BASE_URL}/target/prepare?target=onnx&source_url=file%3A%2F%2F%2Fetc%2Fpasswd" \
    "source_url must be http(s)"
  check_status 400 POST "${BASE_URL}/target/build/start?target=onnx&version=..%2F..%2F..%2Ftmp%2Fpwn" \
    "A16 traversal in build version"
  check_status 400 POST "${BASE_URL}/replay/start?target=onnx&input=%2Fetc%2Fpasswd" \
    "A14 replay input outside the data dir"
  check_status 400 POST "${BASE_URL}/replay/start?target=onnx&triage_id=..%2F..%2Fetc" \
    "A14 triage id must not leave the triage tree"
  # A handler error used to close the connection with zero bytes (curl exit 52) instead of a status.
  check_status 500 GET "${BASE_URL}/file?path=%2Fetc%2Fpasswd" "a refused file view answers a status"
  local state="$WORKDIR/data/ui-target/prepare-target.state"
  if [[ -f "$state" ]] && grep -qx 'pid=1234' "$state"; then
    echo "[FAIL] A2 injected a pid line into $state" | tee -a "$CHECK_LOG"
    return 1
  fi
  echo "[OK] no injected pid line in the prepare state" | tee -a "$CHECK_LOG"
}

check_rejected_inputs

# A4: a client that opens the socket and sends nothing used to block the single-threaded accept
# loop. A client that dribbles a partial head never trips the per-read timeout, so it also has to
# be bounded. In both cases the dashboard must still answer promptly, not merely eventually.
check_slow_clients_do_not_block() {
  local host="${BIND%:*}"
  local port="${BIND##*:}"
  local rc=0
  # 3 silent connections and 3 that send an unterminated head
  local fds=(9 8 7 6 5 4)
  local i=0
  for fd in "${fds[@]}"; do
    if ! eval "exec ${fd}<>/dev/tcp/${host}/${port}" 2>/dev/null; then
      echo "[FAIL] could not open connection ${fd} to ${BIND}" | tee -a "$CHECK_LOG"
      return 1
    fi
    if [[ "$i" -ge 3 ]]; then
      printf 'GET /healthz HTTP/1.1\r\nHost: %s\r\n' "$BIND" >&"$fd" || true
    fi
    i=$((i + 1))
  done
  if curl -fsS --max-time 2 "${BASE_URL}/healthz" >/dev/null 2>&1; then
    echo "[OK] silent and half-open clients do not block the dashboard" | tee -a "$CHECK_LOG"
  else
    echo "[FAIL] a slow client blocked the dashboard" | tee -a "$CHECK_LOG"
    rc=1
  fi
  for fd in "${fds[@]}"; do
    eval "exec ${fd}<&-" 2>/dev/null || true
    eval "exec ${fd}>&-" 2>/dev/null || true
  done
  return "$rc"
}

check_slow_clients_do_not_block

dash_html="$(curl -fsS --max-time 10 "${BASE_URL}/dashboard.html")"

# A placeholder that survives into the served page means the binary and templates/dashboard.html
# have drifted apart, which silently breaks whatever that placeholder drives.
if printf '%s' "$dash_html" | grep -q '{{'; then
  echo "[FAIL] dashboard.html still carries an unsubstituted placeholder:" | tee -a "$CHECK_LOG"
  printf '%s' "$dash_html" | grep -o '{{[a-z_]*}}' | sort -u | tee -a "$CHECK_LOG"
  exit 1
fi
echo "[OK] every dashboard placeholder is substituted" | tee -a "$CHECK_LOG"
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
