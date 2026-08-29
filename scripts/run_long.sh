#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: run_long.sh --target <onnx|gguf|safetensors> --backend <local-harness|libfuzzer|aflpp> (--hours <N>|--duration-seconds <N>) --tag <TAG> [--corpus-dir <dir>] [--data-dir <dir>] [--workers <n>] [--timeout-sec <n>] [--restart-limit <n>] [--max-jobs <n>]
EOF
}

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=lib/engine_mode.sh
. "$SCRIPT_DIR/lib/engine_mode.sh"

WORKDIR="${WORKDIR:-$PWD}"
DATA_DIR="${DATA_DIR:-$WORKDIR/data}"
# G2/G4: refuse a silent fallback when the run is meant to be native/instrumented.
REQUIRE_NATIVE="${REQUIRE_NATIVE:-0}"
REQUIRE_INSTRUMENTED="${REQUIRE_INSTRUMENTED:-0}"
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
      # Same per-target resolution as ops/scripts/fuzz-loop-libfuzzer.sh: this is the
      # path the campaign runners take, and the two must decide identically or a
      # campaign and its systemd twin fuzz different things under the same label.
      case "$TARGET" in
        onnx) NATIVE_DRIVER="${WORKDIR}/harnesses/libfuzzer/onnxruntime_loader_fuzzer" ;;
        gguf) NATIVE_DRIVER="${WORKDIR}/harnesses/libfuzzer/gguf_loader_fuzzer" ;;
        *)    NATIVE_DRIVER="" ;;
      esac
      if [[ -n "$NATIVE_DRIVER" && -x "$NATIVE_DRIVER" ]]; then
        export TOOL_LIBFUZZER_MODE="native"
        export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && LLVM_PROFILE_FILE={artifact_dir}/${TARGET}-native-%p.profraw ${NATIVE_DRIVER} -artifact_prefix={artifact_dir}/ -max_total_time=5 {corpus_dir} >/dev/null 2>&1"
      else
        export TOOL_LIBFUZZER_MODE="blackbox"
        if [[ -n "$NATIVE_DRIVER" ]]; then
          echo "[run-long] WARN: no native libFuzzer driver at ${NATIVE_DRIVER}; running the black-box tool wrapper. This run is NOT a native libFuzzer run." >&2
        else
          echo "[run-long] WARN: target ${TARGET} has no native libFuzzer driver; running the black-box tool wrapper. This run is NOT a native libFuzzer run." >&2
        fi
        if [[ "$REQUIRE_NATIVE" == "1" ]]; then
          echo "[run-long] REQUIRE_NATIVE=1 is set; refusing to run in black-box mode" >&2
          exit 3
        fi
        export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && TOOL_HARNESS_TOOL=${TOOL_BIN} TOOL_HARNESS_TARGET=${TARGET} TOOL_HARNESS_EXT=${TARGET} ${WORKDIR}/harnesses/libfuzzer/tool_harness_driver -artifact_prefix={artifact_dir}/ -max_total_time=5 {corpus_dir} >/dev/null 2>&1"
      fi
      echo "[run-long] libfuzzer_mode=${TOOL_LIBFUZZER_MODE}"
      echo "[run-long] TOOL_LIBFUZZER_CMD=${TOOL_LIBFUZZER_CMD}"
    else
      echo "[run-long] libfuzzer_mode=${TOOL_LIBFUZZER_MODE:-unlabeled} (TOOL_LIBFUZZER_CMD provided by caller)"
    fi
    ;;
  aflpp)
    if [[ -z "${TOOL_AFLPP_CMD:-}" ]]; then
      NATIVE_ONNX_AFLPP_DRIVER="${WORKDIR}/harnesses/aflpp/onnxruntime_loader_replay"
      ONNX_AFLPP_LD_LIBRARY_PATH="{container_workdir}/data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2/build/Linux/Release:{container_workdir}/data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2/build/cov-o0/RelWithDebInfo:{container_workdir}/data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2/build/cov/RelWithDebInfo"
      # G2: only a binary carrying the AFL++ runtime may drive afl-fuzz without -n.
      if [[ "$TARGET" == "onnx" ]] && has_afl_instrumentation "$NATIVE_ONNX_AFLPP_DRIVER"; then
        export TOOL_AFLPP_MODE="instrumented"
        export TOOL_AFLPP_CMD="docker run --rm {docker_user_flag} {docker_hardening_flags} {docker_readonly_flags} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc \"LD_LIBRARY_PATH=${ONNX_AFLPP_LD_LIBRARY_PATH}:\\\$LD_LIBRARY_PATH AFL_IGNORE_SEED_PROBLEMS=1 afl-fuzz -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- {container_workdir}/harnesses/aflpp/onnxruntime_loader_replay @@ >/dev/null 2>&1\""
      else
        export TOOL_AFLPP_MODE="blackbox_n"
        if [[ -x "$NATIVE_ONNX_AFLPP_DRIVER" ]]; then
          echo "[run-long] WARN: ${NATIVE_ONNX_AFLPP_DRIVER} has no AFL++ instrumentation; falling back to -n black-box mode. This run is NOT coverage-guided." >&2
        else
          echo "[run-long] WARN: no native AFL++ replay binary; running -n black-box mode over 'tool harness'. This run is NOT coverage-guided." >&2
        fi
        if [[ "$REQUIRE_INSTRUMENTED" == "1" ]]; then
          echo "[run-long] REQUIRE_INSTRUMENTED=1 is set; refusing to run without instrumentation" >&2
          exit 3
        fi
        # AFL_CRASH_EXITCODE: in -n mode AFL++ only records signal deaths, but the tool
        # wrapper reports a library crash as exit 4. AFL_IGNORE_SEED_PROBLEMS: a corpus
        # that already contains a crashing seed must not abort the whole run.
        export TOOL_AFLPP_CMD="docker run --rm {docker_user_flag} {docker_hardening_flags} {docker_readonly_flags} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc \"AFL_CRASH_EXITCODE=4 AFL_IGNORE_SEED_PROBLEMS=1 afl-fuzz -n -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- {container_workdir}/target/debug/tool harness --target ${TARGET} --input @@ >/dev/null 2>&1\""
      fi
      echo "[run-long] aflpp_mode=${TOOL_AFLPP_MODE}"
      echo "[run-long] TOOL_AFLPP_CMD=${TOOL_AFLPP_CMD}"
    else
      echo "[run-long] aflpp_mode=${TOOL_AFLPP_MODE:-unlabeled} (TOOL_AFLPP_CMD provided by caller)"
    fi
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
