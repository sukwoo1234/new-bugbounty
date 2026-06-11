#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
TOOL_BIN="${TOOL_BIN:-$PROJECT_ROOT/target/debug/tool}"
DATA_DIR="${DATA_DIR:-$PROJECT_ROOT/data/onnx-libfuzzer-artifact-check}"
CRASH_POC="${ONNX_CRASH_POC:-${ONNX_SIGSEGV_POC:-}}"
REQUIRE_POC=0

SIGSEGV_SHA256="61d1c65cde3c8ba65433229f23704f5163708f001664c5a5cced5e28cc202ac8"

usage() {
  cat <<'EOF'
usage: check_onnx_libfuzzer_crash_artifacts.sh [--require-poc]

PoC files are intentionally not committed. Provide a local known-crash path via:
  ONNX_SIGSEGV_POC=/path/to/crash_protobuf.onnx
or:
  ONNX_CRASH_POC=/path/to/crash_protobuf.onnx

Checks:
  - native libFuzzer harness is built
  - libFuzzer writes a crash artifact under artifact_prefix
  - tool run ingests backend artifacts
  - automatic backend triage reproduces at least one crash

Without --require-poc, a missing PoC env var skips the private artifact check.
With --require-poc, a missing PoC env var or failed check is a hard failure.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --require-poc) REQUIRE_POC=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[onnx-libfuzzer-artifact-check] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

log() {
  echo "[onnx-libfuzzer-artifact-check] $*"
}

fail() {
  echo "[onnx-libfuzzer-artifact-check] fail: $*" >&2
  exit 1
}

json_number_field() {
  local file="$1"
  local key="$2"
  python3 - "$file" "$key" <<'PY'
import json, sys
path, key = sys.argv[1:3]
with open(path, encoding="utf-8") as f:
    data = json.load(f)
value = data.get(key, 0)
print(value if isinstance(value, int) else 0)
PY
}

latest_status() {
  find "$DATA_DIR/runs" -mindepth 2 -maxdepth 2 -type f -name status.json 2>/dev/null \
    | sort \
    | tail -n 1
}

if [[ -z "$CRASH_POC" ]]; then
  if [[ "$REQUIRE_POC" -eq 1 ]]; then
    fail "ONNX_SIGSEGV_POC or ONNX_CRASH_POC is required"
  fi
  log "skip: ONNX_SIGSEGV_POC/ONNX_CRASH_POC not set"
  exit 0
fi

[[ -f "$CRASH_POC" ]] || fail "PoC not found: $CRASH_POC"
actual_sha="$(sha256sum "$CRASH_POC" | awk '{print $1}')"
[[ "$actual_sha" == "$SIGSEGV_SHA256" ]] || fail "PoC sha256 mismatch: got $actual_sha expected $SIGSEGV_SHA256"

if [[ ! -x "$TOOL_BIN" ]]; then
  log "$TOOL_BIN missing; building debug tool"
  cargo build --offline >/tmp/onnx-libfuzzer-artifact-cargo-build.log
fi

log "build native libFuzzer harness"
"$PROJECT_ROOT/scripts/build_libfuzzer_onnx_native.sh" >/tmp/onnx-libfuzzer-artifact-build.log

rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"
corpus_dir="$(mktemp -d "$DATA_DIR/corpus.XXXXXX")"
cp "$CRASH_POC" "$corpus_dir/crash_protobuf.onnx"

export TOOL_LIBFUZZER_CMD='mkdir -p {artifact_dir} && LLVM_PROFILE_FILE={artifact_dir}/onnx-native-%p.profraw '"$PROJECT_ROOT"'/harnesses/libfuzzer/onnxruntime_loader_fuzzer -artifact_prefix={artifact_dir}/ -runs=1 {corpus_dir} >/dev/null 2>&1'

log "run backend libFuzzer with known-crash corpus"
set +e
"$TOOL_BIN" --data-dir "$DATA_DIR" run \
  --target onnx \
  --backend libfuzzer \
  --corpus-dir "$corpus_dir" \
  --workers 1 \
  --timeout-sec 30 \
  --restart-limit 0 \
  >"$DATA_DIR/tool-run.log" 2>&1
run_rc=$?
set -e

if [[ "$run_rc" -eq 0 ]]; then
  fail "tool run unexpectedly succeeded for known-crash libFuzzer corpus"
fi

status="$(latest_status)"
[[ -n "$status" && -f "$status" ]] || { cat "$DATA_DIR/tool-run.log" >&2 || true; fail "status.json not found"; }

artifacts="$(json_number_field "$status" backend_crash_artifacts)"
triaged="$(json_number_field "$status" backend_crashes_triaged)"
errors="$(json_number_field "$status" backend_crash_triage_errors)"

[[ "$artifacts" -ge 1 ]] || { cat "$DATA_DIR/tool-run.log" >&2 || true; fail "backend_crash_artifacts=$artifacts status=$status"; }
[[ "$triaged" -ge 1 ]] || { cat "$DATA_DIR/tool-run.log" >&2 || true; fail "backend_crashes_triaged=$triaged status=$status"; }
[[ "$errors" -eq 0 ]] || { cat "$DATA_DIR/tool-run.log" >&2 || true; fail "backend_crash_triage_errors=$errors status=$status"; }

log "done: artifacts=$artifacts triaged=$triaged status=$status"
