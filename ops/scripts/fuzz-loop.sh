#!/usr/bin/env bash
# Continuous ONNX fuzzing wrapper for 1-week systemd run on 퍼징컴.
# Decision context: docs/plans/session-handoff.md ⑦(c).
#
# Loop: mutate (50 inputs) -> run (workers 12, timeout 30s) -> sleep 2s -> next.
# Graceful stop: SIGTERM finishes current run, then exits.
# Cleanup: keep newest MAX_BATCHES_KEEP, delete the rest.
# Smoke: FUZZ_LOOP_MAX_ITERATIONS=N exits after N iterations (test only).

set -uo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-/home/ssw/bugbounty}"
TARGET="${TARGET:-onnx}"
BACKEND="${BACKEND:-local-harness}"
WORKERS="${WORKERS:-12}"
TIMEOUT_SEC="${TIMEOUT_SEC:-30}"
RESTART_LIMIT="${RESTART_LIMIT:-1}"
SEED_DIR="${SEED_DIR:-${PROJECT_ROOT}/seeds/${TARGET}}"
MUTATE_COUNT="${MUTATE_COUNT:-50}"
ITERATION_SLEEP_SEC="${ITERATION_SLEEP_SEC:-2}"
MAX_BATCHES_KEEP="${MAX_BATCHES_KEEP:-2000}"
MAX_ITERATIONS="${FUZZ_LOOP_MAX_ITERATIONS:-0}"

MUTATED_ROOT="${PROJECT_ROOT}/data/corpus/mutated/${TARGET}"
TOOL_BIN="${PROJECT_ROOT}/target/release/tool"

log() {
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] fuzz-loop: $*"
}

cd "${PROJECT_ROOT}"
mkdir -p "${MUTATED_ROOT}"

stop_requested=0
trap 'stop_requested=1; log "SIGTERM received, will exit after current iteration"' TERM INT

# Resume iteration counter from existing batches (restart continuity).
last_iter=$(ls -1 "${MUTATED_ROOT}" 2>/dev/null \
    | grep -oE 'iter[0-9]{6}' \
    | sort -u \
    | tail -1 \
    | sed 's/iter//; s/^0*//')
iter=${last_iter:-0}
log "starting fuzz-loop (resume from iter=${iter}, target=${TARGET}, backend=${BACKEND}, workers=${WORKERS})"

while :; do
    if [ "${stop_requested}" = "1" ]; then
        log "stop_requested, exiting at iter=${iter}"
        exit 0
    fi

    iter=$((iter + 1))
    iter_str=$(printf 'iter%06d' "${iter}")
    ts=$(date -u +%Y%m%d-%H%M%S)
    batch_id="batch-${iter_str}-${ts}"
    batch_dir="${MUTATED_ROOT}/${batch_id}"

    log "iter=${iter} starting batch ${batch_id}"

    if ! "${TOOL_BIN}" mutate \
        --target "${TARGET}" \
        --input-dir "${SEED_DIR}" \
        --out-dir "${batch_dir}" \
        --count "${MUTATE_COUNT}" \
        --seed "${iter}"
    then
        log "iter=${iter} mutate failed, skipping run"
        rm -rf "${batch_dir}"
        sleep "${ITERATION_SLEEP_SEC}"
        continue
    fi

    if ! "${TOOL_BIN}" run \
        --target "${TARGET}" \
        --backend "${BACKEND}" \
        --workers "${WORKERS}" \
        --timeout-sec "${TIMEOUT_SEC}" \
        --restart-limit "${RESTART_LIMIT}" \
        --corpus-dir "${batch_dir}"
    then
        log "iter=${iter} run failed, continuing to next iteration"
    fi

    batch_count=$(ls -1 "${MUTATED_ROOT}" 2>/dev/null | grep -c '^batch-iter' || true)
    if [ "${batch_count}" -gt "${MAX_BATCHES_KEEP}" ]; then
        excess=$((batch_count - MAX_BATCHES_KEEP))
        ls -1 "${MUTATED_ROOT}" 2>/dev/null \
            | grep '^batch-iter' \
            | sort \
            | head -n "${excess}" \
            | while IFS= read -r old; do
                rm -rf "${MUTATED_ROOT:?}/${old}"
            done
        log "cleaned ${excess} old batches (kept newest ${MAX_BATCHES_KEEP})"
    fi

    if [ "${MAX_ITERATIONS}" -ne 0 ] && [ "${iter}" -ge "${MAX_ITERATIONS}" ]; then
        log "reached MAX_ITERATIONS=${MAX_ITERATIONS}, exiting"
        exit 0
    fi

    sleep "${ITERATION_SLEEP_SEC}"
done
