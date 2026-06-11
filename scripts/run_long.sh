#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: run_long.sh --target <onnx|gguf|safetensors> --backend <local-harness|libfuzzer|aflpp> (--hours <N>|--duration-seconds <N>) --tag <TAG> [--corpus-dir <dir>] [--data-dir <dir>] [--workers <n>] [--timeout-sec <n>] [--restart-limit <n>] [--max-jobs <n>]
EOF
}

WORKDIR="${WORKDIR:-$PWD}"
DATA_DIR="${DATA_DIR:-$WORKDIR/data}"
TOOL_BIN="${TOOL_BIN:-$WORKDIR/target/debug/tool}"
TARGET=""
BACKEND=""
HOURS=""
DURATION_SECONDS=""
TAG=""
CORPUS_DIR=""
WORKERS="1"
TIMEOUT_SEC="30"
RESTART_LIMIT="1"
MAX_JOBS=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="${2:-}"; shift 2 ;;
    --backend) BACKEND="${2:-}"; shift 2 ;;
    --hours) HOURS="${2:-}"; shift 2 ;;
    --duration-seconds) DURATION_SECONDS="${2:-}"; shift 2 ;;
    --tag) TAG="${2:-}"; shift 2 ;;
    --corpus-dir) CORPUS_DIR="${2:-}"; shift 2 ;;
    --data-dir) DATA_DIR="${2:-}"; shift 2 ;;
    --workers) WORKERS="${2:-}"; shift 2 ;;
    --timeout-sec) TIMEOUT_SEC="${2:-}"; shift 2 ;;
    --restart-limit) RESTART_LIMIT="${2:-}"; shift 2 ;;
    --max-jobs) MAX_JOBS="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[run-long] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ -z "$TARGET" || -z "$BACKEND" || -z "$TAG" ]]; then
  echo "[run-long] required args missing" >&2
  usage
  exit 2
fi
if [[ -z "$HOURS" && -z "$DURATION_SECONDS" ]]; then
  echo "[run-long] either --hours or --duration-seconds is required" >&2
  usage
  exit 2
fi
if [[ -n "$HOURS" && -n "$DURATION_SECONDS" ]]; then
  echo "[run-long] use only one of --hours or --duration-seconds" >&2
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
    if [[ -z "${TOOL_LIBFUZZER_CMD:-}" ]]; then
      NATIVE_ONNX_DRIVER="${WORKDIR}/harnesses/libfuzzer/onnxruntime_loader_fuzzer"
      if [[ "$TARGET" == "onnx" && -x "$NATIVE_ONNX_DRIVER" ]]; then
        export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && LLVM_PROFILE_FILE={artifact_dir}/onnx-native-%p.profraw ${NATIVE_ONNX_DRIVER} -artifact_prefix={artifact_dir}/ -max_total_time=5 {corpus_dir} >/dev/null 2>&1"
      else
        export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && TOOL_HARNESS_TOOL=${TOOL_BIN} TOOL_HARNESS_TARGET=${TARGET} TOOL_HARNESS_EXT=${TARGET} ${WORKDIR}/harnesses/libfuzzer/tool_harness_driver -artifact_prefix={artifact_dir}/ -max_total_time=5 {corpus_dir} >/dev/null 2>&1"
      fi
    fi
    ;;
  aflpp)
    export TOOL_AFLPP_CMD="docker run --rm {docker_user_flag} {docker_hardening_flags} {docker_readonly_flags} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc \"afl-fuzz -n -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- {container_workdir}/target/debug/tool harness --target ${TARGET} --input @@ >/dev/null 2>&1\""
    ;;
  *)
    echo "[run-long] unsupported backend: $BACKEND" >&2
    exit 2
    ;;
esac

if [[ -n "$DURATION_SECONDS" ]]; then
  DURATION_LABEL="${DURATION_SECONDS}s"
else
  DURATION_LABEL="${HOURS}h"
fi

echo "[run-long] target=$TARGET backend=$BACKEND duration=$DURATION_LABEL tag=$TAG corpus=$CORPUS_DIR data_dir=$DATA_DIR workers=$WORKERS timeout=$TIMEOUT_SEC restart=$RESTART_LIMIT max_jobs=${MAX_JOBS:-all}"

TAG="$TAG" \
WORKDIR="$WORKDIR" \
DATA_DIR="$DATA_DIR" \
TOOL_BIN="$TOOL_BIN" \
TARGET="$TARGET" \
BACKEND="$BACKEND" \
CORPUS_DIR="$CORPUS_DIR" \
WORKERS="$WORKERS" \
TIMEOUT_SEC="$TIMEOUT_SEC" \
RESTART_LIMIT="$RESTART_LIMIT" \
MAX_JOBS="$MAX_JOBS" \
DURATION_HOURS="$HOURS" \
DURATION_SECONDS="$DURATION_SECONDS" \
bash "$WORKDIR/scripts/run_backend_loop.sh"
