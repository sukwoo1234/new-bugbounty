#!/usr/bin/env bash
# Continuous ONNX libfuzzer fuzzing wrapper for 1-week systemd run on 퍼징컴.
# Decision context: results/experiments/2026-05-19-onnx-local-harness-1week-06-211-01/notes.md §"Suggested Follow-up".
#
# Difference vs ops/scripts/fuzz-loop.sh (local-harness):
#   - libfuzzer manages its own corpus + mutation internally, so no `tool mutate` step.
#   - Loop is just: `tool run --backend libfuzzer` → sleep → repeat.
#   - TOOL_LIBFUZZER_CMD env var must be set; this wrapper exports a default.
#
# Loop: run (workers 12, timeout 30s) -> sleep 2s -> next.
# Graceful stop: SIGTERM finishes current run, then exits.
# Smoke: FUZZ_LOOP_MAX_ITERATIONS=N exits after N iterations (test only).

set -uo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-/home/ssw/bugbounty}"
TARGET="${TARGET:-onnx}"
BACKEND="${BACKEND:-libfuzzer}"
WORKERS="${WORKERS:-12}"
TIMEOUT_SEC="${TIMEOUT_SEC:-30}"
RESTART_LIMIT="${RESTART_LIMIT:-1}"
CORPUS_DIR="${CORPUS_DIR:-${PROJECT_ROOT}/seeds/${TARGET}}"
ITERATION_SLEEP_SEC="${ITERATION_SLEEP_SEC:-2}"
MAX_ITERATIONS="${FUZZ_LOOP_MAX_ITERATIONS:-0}"

LIBFUZZER_MAX_TOTAL_TIME="${LIBFUZZER_MAX_TOTAL_TIME:-30}"
NATIVE_ONNX_DRIVER="${PROJECT_ROOT}/harnesses/libfuzzer/onnxruntime_loader_fuzzer"
TOOL_DRIVER="${PROJECT_ROOT}/harnesses/libfuzzer/tool_harness_driver"

if [[ -z "${LIBFUZZER_DRIVER:-}" ]]; then
    if [[ "${TARGET}" == "onnx" && -x "${NATIVE_ONNX_DRIVER}" ]]; then
        LIBFUZZER_DRIVER="${NATIVE_ONNX_DRIVER}"
    else
        LIBFUZZER_DRIVER="${TOOL_DRIVER}"
    fi
fi

TOOL_BIN="${PROJECT_ROOT}/target/release/tool"

log() {
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] fuzz-loop-libfuzzer: $*"
}

cd "${PROJECT_ROOT}"

stop_requested=0
trap 'stop_requested=1; log "SIGTERM received, will exit after current iteration"' TERM INT

# libfuzzer driver invocation contract (matches docs/experiment-ops.md §libfuzzer):
#   {corpus_dir} is substituted by `tool run --backend libfuzzer` with the chosen
#   workdir-local libfuzzer corpus path. {artifact_dir} is a per-worker run dir
#   for libFuzzer crash artifacts. -max_total_time bounds per-invocation runtime.
if [[ "${TARGET}" == "onnx" && "${LIBFUZZER_DRIVER}" == "${NATIVE_ONNX_DRIVER}" ]]; then
    export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && LLVM_PROFILE_FILE={artifact_dir}/onnx-native-%p.profraw ${LIBFUZZER_DRIVER} -artifact_prefix={artifact_dir}/ -max_total_time=${LIBFUZZER_MAX_TOTAL_TIME} {corpus_dir} >/dev/null 2>&1"
else
    export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && TOOL_HARNESS_TOOL=${TOOL_BIN} TOOL_HARNESS_TARGET=${TARGET} TOOL_HARNESS_EXT=${TARGET} ${LIBFUZZER_DRIVER} -artifact_prefix={artifact_dir}/ -max_total_time=${LIBFUZZER_MAX_TOTAL_TIME} {corpus_dir} >/dev/null 2>&1"
fi

iter=0
log "starting libfuzzer loop (target=${TARGET}, workers=${WORKERS}, libfuzzer max_total_time=${LIBFUZZER_MAX_TOTAL_TIME}s, corpus=${CORPUS_DIR})"
log "TOOL_LIBFUZZER_CMD=${TOOL_LIBFUZZER_CMD}"

while :; do
    if [ "${stop_requested}" = "1" ]; then
        log "stop_requested, exiting at iter=${iter}"
        exit 0
    fi

    iter=$((iter + 1))
    log "iter=${iter} starting"

    if ! "${TOOL_BIN}" run \
        --target "${TARGET}" \
        --backend "${BACKEND}" \
        --workers "${WORKERS}" \
        --timeout-sec "${TIMEOUT_SEC}" \
        --restart-limit "${RESTART_LIMIT}" \
        --corpus-dir "${CORPUS_DIR}"
    then
        log "iter=${iter} run failed, continuing to next iteration"
    fi

    if [ "${MAX_ITERATIONS}" -ne 0 ] && [ "${iter}" -ge "${MAX_ITERATIONS}" ]; then
        log "reached MAX_ITERATIONS=${MAX_ITERATIONS}, exiting"
        exit 0
    fi

    sleep "${ITERATION_SLEEP_SEC}"
done
