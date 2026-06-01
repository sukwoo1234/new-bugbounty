#!/usr/bin/env bash
# Continuous ONNX AFL++ fuzzing wrapper for 1-week systemd run on 퍼징컴 (Arm C).
# Decision context: results/experiments/2026-05-19-onnx-local-harness-1week-06-211-01/notes.md §"Suggested Follow-up"
# and the libfuzzer (Arm B) ops pair ops/scripts/fuzz-loop-libfuzzer.sh / ops/systemd/tool-fuzz-onnx-libfuzzer.service.
#
# Difference vs ops/scripts/fuzz-loop-libfuzzer.sh (Arm B):
#   - AFL++ runs as a hardened Docker container (aflplusplus/aflplusplus) in `-n` dumb/black-box mode.
#     The container drives the uninstrumented `tool harness` against @@ inputs — onnxruntime is NOT
#     instrumented (same black-box framing as the libfuzzer driver). No native afl-fuzz install needed.
#   - TOOL_AFLPP_CMD env var must be set; this wrapper exports the hardened template (matches
#     scripts/run_long.sh aflpp leg + docs/specs.md §Docker hardening). run.rs substitutes the
#     {docker_*}/{workdir_abs}/{corpus_dir_abs}/{run_dir_abs}/{container_*} placeholders at run time.
#   - WORKERS defaults to 2 (NOT 12): each container reserves --cpus 2 + --memory 4g, so on the
#     퍼징컴 (i7-10700 8C/16T, 16GB) 2 workers ≈ 4 vCPU / 8GB. Day-1 RAM > 85% or swap > 0 → lower to 1.
#
# Loop: run (workers 2, each container -V 5) -> sleep 2s -> next.
# Graceful stop: SIGTERM finishes current run, then exits.
# Smoke: FUZZ_LOOP_MAX_ITERATIONS=N exits after N iterations (test only).
#
# Prerequisites on 퍼징컴:
#   - docker.io installed; service user in the `docker` group (so afl-out is not root-owned;
#     {docker_user_flag} also enforces uid:gid). Verify: `docker run --rm aflplusplus/aflplusplus afl-fuzz -h`.
#   - target/release/tool built (orchestrator) AND the in-container tool built (default target/debug/tool;
#     override with AFLPP_CONTAINER_TOOL). i.e. on 퍼징컴: `cargo build --release && cargo build`.
#   - .venv with onnxruntime present under the workdir (mounted /work:ro) — Docker runs with
#     --network none, so onnxruntime cannot be pip-installed at runtime; it must already be present.

set -uo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-/home/ssw/bugbounty}"
TARGET="${TARGET:-onnx}"
BACKEND="${BACKEND:-aflpp}"
WORKERS="${WORKERS:-2}"
TIMEOUT_SEC="${TIMEOUT_SEC:-30}"
RESTART_LIMIT="${RESTART_LIMIT:-1}"
CORPUS_DIR="${CORPUS_DIR:-${PROJECT_ROOT}/seeds/${TARGET}}"
ITERATION_SLEEP_SEC="${ITERATION_SLEEP_SEC:-2}"
MAX_ITERATIONS="${FUZZ_LOOP_MAX_ITERATIONS:-0}"

# In-container target binary path (relative to {container_workdir}=/work). The 1h/6h aflpp
# validation used target/debug/tool; override to target/release/tool if only release is built.
AFLPP_CONTAINER_TOOL="${AFLPP_CONTAINER_TOOL:-target/debug/tool}"

TOOL_BIN="${PROJECT_ROOT}/target/release/tool"

log() {
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] fuzz-loop-aflpp: $*"
}

cd "${PROJECT_ROOT}"

stop_requested=0
trap 'stop_requested=1; log "SIGTERM received, will exit after current iteration"' TERM INT

# AFL++ docker invocation contract (hardened; matches scripts/run_long.sh aflpp leg and
# docs/specs.md §Docker hardening). Placeholders are substituted by `tool run --backend aflpp`:
#   {docker_user_flag}      = --user uid:gid (afl-out ownership)
#   {docker_hardening_flags}= --network none --memory 4g --cpus 2 --pids-limit 512
#   {docker_readonly_flags} = --read-only --tmpfs /tmp:rw,size=1g --tmpfs /dev/shm:rw,size=1g
#   {workdir_abs}/{corpus_dir_abs}/{run_dir_abs} = host mounts; {container_*} = in-container paths
export TOOL_AFLPP_CMD="docker run --rm {docker_user_flag} {docker_hardening_flags} {docker_readonly_flags} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc \"afl-fuzz -n -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- {container_workdir}/${AFLPP_CONTAINER_TOOL} harness --target ${TARGET} --input @@ >/dev/null 2>&1 || true\""

iter=0
log "starting aflpp loop (target=${TARGET}, workers=${WORKERS}, container_tool=${AFLPP_CONTAINER_TOOL}, corpus=${CORPUS_DIR})"
log "TOOL_AFLPP_CMD=${TOOL_AFLPP_CMD}"

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
