#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: run_long.sh --target <onnx|gguf|safetensors> --backend <local-harness|libfuzzer|aflpp> --hours <N> --tag <TAG> [--corpus-dir <dir>] [--workers <n>] [--timeout-sec <n>] [--restart-limit <n>]
EOF
}

TARGET=""
BACKEND=""
HOURS=""
TAG=""
CORPUS_DIR=""
WORKERS="1"
TIMEOUT_SEC="30"
RESTART_LIMIT="1"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="${2:-}"; shift 2 ;;
    --backend) BACKEND="${2:-}"; shift 2 ;;
    --hours) HOURS="${2:-}"; shift 2 ;;
    --tag) TAG="${2:-}"; shift 2 ;;
    --corpus-dir) CORPUS_DIR="${2:-}"; shift 2 ;;
    --workers) WORKERS="${2:-}"; shift 2 ;;
    --timeout-sec) TIMEOUT_SEC="${2:-}"; shift 2 ;;
    --restart-limit) RESTART_LIMIT="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[run-long] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ -z "$TARGET" || -z "$BACKEND" || -z "$HOURS" || -z "$TAG" ]]; then
  echo "[run-long] required args missing" >&2
  usage
  exit 2
fi

if [[ -z "$CORPUS_DIR" ]]; then
  CORPUS_DIR="seeds/${TARGET}"
fi

if [[ ! -d "$CORPUS_DIR" ]]; then
  echo "[run-long] corpus dir not found: $CORPUS_DIR" >&2
  exit 2
fi

case "$BACKEND" in
  local-harness)
    ;;
  libfuzzer)
    export TOOL_LIBFUZZER_CMD="TOOL_HARNESS_TOOL=./target/debug/tool TOOL_HARNESS_TARGET=${TARGET} TOOL_HARNESS_EXT=${TARGET} ./harnesses/libfuzzer/tool_harness_driver -max_total_time=5 {corpus_dir} >/dev/null 2>&1"
    ;;
  aflpp)
    export TOOL_AFLPP_CMD="docker run --rm {docker_user_flag} {docker_hardening_flags} -v \"\$PWD\":/work -w /work aflplusplus/aflplusplus bash -lc \"afl-fuzz -n -V 5 -i {corpus_dir} -o {run_dir}/afl-out -- /work/target/debug/tool harness --target ${TARGET} --input @@ >/dev/null 2>&1 || true\""
    ;;
  *)
    echo "[run-long] unsupported backend: $BACKEND" >&2
    exit 2
    ;;
esac

echo "[run-long] target=$TARGET backend=$BACKEND hours=$HOURS tag=$TAG corpus=$CORPUS_DIR workers=$WORKERS timeout=$TIMEOUT_SEC restart=$RESTART_LIMIT"

TAG="$TAG" \
TARGET="$TARGET" \
BACKEND="$BACKEND" \
CORPUS_DIR="$CORPUS_DIR" \
WORKERS="$WORKERS" \
TIMEOUT_SEC="$TIMEOUT_SEC" \
RESTART_LIMIT="$RESTART_LIMIT" \
DURATION_HOURS="$HOURS" \
bash scripts/run_backend_loop.sh
