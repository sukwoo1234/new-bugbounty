#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: run_onnx_abc_week.sh [options]

Run a controlled ONNX A/B/C comparison campaign:
  Arm A: local-harness with aggressive ONNX mutations
  Arm B: native ONNX libFuzzer harness
  Arm C: native ONNX AFL++ replay harness

Defaults are conservative for WSL fuzzing hosts:
  7 days, 8h per arm block, rotated arm order, isolated data dirs.

Options:
  --campaign-id <id>              Default: onnx-abc-week-YYYYmmdd-HHMMSS
  --days <n>                      Default: 7
  --block-hours <n>               Default: 8
  --block-seconds <n>             Override block duration for pilot/smoke runs
  --data-root <dir>               Default: data/campaigns
  --corpus-dir <dir>              Default: seeds/onnx
  --workers-local <n>             Default: 2
  --workers-libfuzzer <n>         Default: 2
  --workers-aflpp <n>             Default: 1
  --timeout-sec <n>               Default: 30
  --restart-limit <n>             Default: 1
  --mutate-count <n>              Default: 50
  --mutate-operators <ops>        Default: aggressive
  --libfuzzer-max-total-time <n>  Default: 30
  --monitor-interval-sec <n>      Default: 300
  --min-free-gb <n>               Default: 50
  --skip-preflight                Skip host preflight checks
  --dry-run                       Print planned blocks without running fuzzers
  -h, --help                      Show this help
EOF
}

WORKDIR="${WORKDIR:-$PWD}"
TOOL_BIN="${TOOL_BIN:-$WORKDIR/target/release/tool}"
CAMPAIGN_ID="onnx-abc-week-$(date +%Y%m%d-%H%M%S)"
DAYS="7"
BLOCK_HOURS="8"
BLOCK_SECONDS=""
DATA_ROOT="data/campaigns"
CORPUS_DIR="seeds/onnx"
WORKERS_LOCAL="2"
WORKERS_LIBFUZZER="2"
WORKERS_AFLPP="1"
TIMEOUT_SEC="30"
RESTART_LIMIT="1"
MUTATE_COUNT="50"
MUTATE_OPERATORS="aggressive"
LIBFUZZER_MAX_TOTAL_TIME="30"
MONITOR_INTERVAL_SEC="300"
MIN_FREE_GB="50"
SKIP_PREFLIGHT="0"
DRY_RUN="0"
TARGET="onnx"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --campaign-id) CAMPAIGN_ID="${2:-}"; shift 2 ;;
    --days) DAYS="${2:-}"; shift 2 ;;
    --block-hours) BLOCK_HOURS="${2:-}"; shift 2 ;;
    --block-seconds) BLOCK_SECONDS="${2:-}"; shift 2 ;;
    --data-root) DATA_ROOT="${2:-}"; shift 2 ;;
    --corpus-dir) CORPUS_DIR="${2:-}"; shift 2 ;;
    --workers-local) WORKERS_LOCAL="${2:-}"; shift 2 ;;
    --workers-libfuzzer) WORKERS_LIBFUZZER="${2:-}"; shift 2 ;;
    --workers-aflpp) WORKERS_AFLPP="${2:-}"; shift 2 ;;
    --timeout-sec) TIMEOUT_SEC="${2:-}"; shift 2 ;;
    --restart-limit) RESTART_LIMIT="${2:-}"; shift 2 ;;
    --mutate-count) MUTATE_COUNT="${2:-}"; shift 2 ;;
    --mutate-operators) MUTATE_OPERATORS="${2:-}"; shift 2 ;;
    --libfuzzer-max-total-time) LIBFUZZER_MAX_TOTAL_TIME="${2:-}"; shift 2 ;;
    --monitor-interval-sec) MONITOR_INTERVAL_SEC="${2:-}"; shift 2 ;;
    --min-free-gb) MIN_FREE_GB="${2:-}"; shift 2 ;;
    --skip-preflight) SKIP_PREFLIGHT="1"; shift ;;
    --dry-run) DRY_RUN="1"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[onnx-abc-week] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

require_uint() {
  local name="$1"
  local value="$2"
  local min="$3"
  if [[ ! "$value" =~ ^[0-9]+$ || "$value" -lt "$min" ]]; then
    echo "[onnx-abc-week] $name must be an integer >= $min" >&2
    exit 2
  fi
}

require_id() {
  local name="$1"
  local value="$2"
  if [[ ! "$value" =~ ^[A-Za-z0-9._-]+$ ]]; then
    echo "[onnx-abc-week] $name allows only A-Z, a-z, 0-9, dot, underscore, dash" >&2
    exit 2
  fi
}

require_uint "--days" "$DAYS" 1
require_uint "--block-hours" "$BLOCK_HOURS" 1
if [[ -n "$BLOCK_SECONDS" ]]; then
  require_uint "--block-seconds" "$BLOCK_SECONDS" 1
fi
require_uint "--workers-local" "$WORKERS_LOCAL" 1
require_uint "--workers-libfuzzer" "$WORKERS_LIBFUZZER" 1
require_uint "--workers-aflpp" "$WORKERS_AFLPP" 1
require_uint "--timeout-sec" "$TIMEOUT_SEC" 1
require_uint "--restart-limit" "$RESTART_LIMIT" 0
require_uint "--mutate-count" "$MUTATE_COUNT" 1
require_uint "--libfuzzer-max-total-time" "$LIBFUZZER_MAX_TOTAL_TIME" 1
require_uint "--monitor-interval-sec" "$MONITOR_INTERVAL_SEC" 1
require_uint "--min-free-gb" "$MIN_FREE_GB" 0
require_id "--campaign-id" "$CAMPAIGN_ID"

abs_path() {
  local path="$1"
  if [[ "$path" = /* ]]; then
    printf '%s' "$path"
  else
    printf '%s/%s' "$WORKDIR" "$path"
  fi
}

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  printf '%s' "$value"
}

log() {
  echo "[$(date -Iseconds)] onnx-abc-week: $*"
}

free_gb_for_path() {
  local path="$1"
  local probe="$path"
  while [[ ! -e "$probe" && "$probe" != "/" ]]; do
    probe="$(dirname "$probe")"
  done
  df -Pk "$probe" | awk 'NR==2 { printf "%d", $4 / 1024 / 1024 }'
}

git_value() {
  local fallback="$1"
  shift
  git -C "$WORKDIR" "$@" 2>/dev/null || printf '%s\n' "$fallback"
}

find_ort_src() {
  local candidate
  for candidate in \
    "${ORT_SRC:-}" \
    "$WORKDIR/data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2" \
    "$WORKDIR/data/targets/onnxruntime/v1.23.2/build-src/onnxruntime-1.23.2"
  do
    if [[ -n "$candidate" && -f "$candidate/build/Linux/Release/libonnxruntime.so" ]]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

host_snapshot() {
  local out="$1"
  {
    echo "===== $(date -Iseconds) ====="
    echo "--- uptime ---"
    uptime || true
    echo "--- memory ---"
    free -h || true
    echo "--- swaps ---"
    cat /proc/swaps 2>/dev/null || true
    echo "--- disk ---"
    df -h "$WORKDIR" "$CAMPAIGN_ROOT" 2>/dev/null || true
    echo "--- top fuzz processes ---"
    ps -eo pid,etimes,pcpu,pmem,cmd 2>/dev/null \
      | grep -E 'tool|fuzz|afl|libfuzzer|onnxruntime|python' \
      | grep -v grep \
      | head -40 || true
    echo
  } >> "$out"
}

monitor_loop() {
  local out="$1"
  while :; do
    host_snapshot "$out"
    sleep "$MONITOR_INTERVAL_SEC"
  done
}

write_manifest() {
  cat > "$MANIFEST_PATH" <<EOF
{
  "schema_version": "1.0",
  "campaign_id": "$(json_escape "$CAMPAIGN_ID")",
  "target": "$TARGET",
  "design": "rotated_serial_abc_blocks",
  "days": $DAYS,
  "block_seconds": $BLOCK_DURATION_SECONDS,
  "block_hours_requested": $BLOCK_HOURS,
  "workdir": "$(json_escape "$WORKDIR")",
  "data_root": "$(json_escape "$DATA_ROOT_ABS")",
  "campaign_root": "$(json_escape "$CAMPAIGN_ROOT")",
  "corpus_dir": "$(json_escape "$CORPUS_DIR_ABS")",
  "tool_bin": "$(json_escape "$TOOL_BIN")",
  "git_commit": "$(json_escape "$GIT_COMMIT")",
  "git_branch": "$(json_escape "$GIT_BRANCH")",
  "ort_src": "$(json_escape "$ORT_SRC_RESOLVED")",
  "so_dir": "$(json_escape "$SO_DIR_RESOLVED")",
  "timeout_sec": $TIMEOUT_SEC,
  "restart_limit": $RESTART_LIMIT,
  "mutate_count": $MUTATE_COUNT,
  "mutate_operators": "$(json_escape "$MUTATE_OPERATORS")",
  "libfuzzer_max_total_time": $LIBFUZZER_MAX_TOTAL_TIME,
  "arms": [
    {"arm": "A", "backend": "local-harness", "workers": $WORKERS_LOCAL, "notes": "aggressive ONNX mutation loop"},
    {"arm": "B", "backend": "libfuzzer", "workers": $WORKERS_LIBFUZZER, "notes": "native ONNX libFuzzer harness"},
    {"arm": "C", "backend": "aflpp", "workers": $WORKERS_AFLPP, "notes": "native ONNX AFL++ replay harness"}
  ],
  "started_at": "$(date -Iseconds)"
}
EOF
}

write_status_json() {
  local state="$1"
  local current="$2"
  local completed="$3"
  cat > "$STATUS_JSON" <<EOF
{
  "schema_version": "1.0",
  "campaign_id": "$(json_escape "$CAMPAIGN_ID")",
  "state": "$(json_escape "$state")",
  "current": "$(json_escape "$current")",
  "completed_blocks": $completed,
  "total_blocks": $TOTAL_BLOCKS,
  "updated_at": "$(date -Iseconds)",
  "status_tsv": "$(json_escape "$STATUS_TSV")",
  "host_log": "$(json_escape "$HOST_LOG")"
}
EOF
}

arm_backend() {
  case "$1" in
    A) printf 'local-harness' ;;
    B) printf 'libfuzzer' ;;
    C) printf 'aflpp' ;;
    *) return 1 ;;
  esac
}

arm_workers() {
  case "$1" in
    A) printf '%s' "$WORKERS_LOCAL" ;;
    B) printf '%s' "$WORKERS_LIBFUZZER" ;;
    C) printf '%s' "$WORKERS_AFLPP" ;;
    *) return 1 ;;
  esac
}

arm_for_slot() {
  local day_index="$1"
  local slot="$2"
  local arms=(A B C)
  local index=$(( (day_index + slot) % 3 ))
  printf '%s' "${arms[$index]}"
}

run_local_block() {
  local data_dir="$1"
  local block_index="$2"
  local end_epoch="$3"
  local mutated_root="$data_dir/corpus/mutated/$TARGET"
  local iter=0
  local mutate_args=()

  mkdir -p "$mutated_root"
  if [[ -n "$MUTATE_OPERATORS" ]]; then
    IFS=',' read -r -a raw_ops <<< "$MUTATE_OPERATORS"
    for raw_op in "${raw_ops[@]}"; do
      local op
      op="${raw_op//[[:space:]]/}"
      if [[ -n "$op" ]]; then
        mutate_args+=(--operator "$op")
      fi
    done
  fi

  while [[ "$(date +%s)" -lt "$end_epoch" ]]; do
    iter=$((iter + 1))
    local iter_str
    local batch_dir
    iter_str="$(printf 'block%03d-iter%06d' "$block_index" "$iter")"
    batch_dir="$mutated_root/batch-${iter_str}-$(date -u +%Y%m%d-%H%M%S)"

    log "Arm A local block=${block_index} iter=${iter} mutate -> $batch_dir"
    if ! "$TOOL_BIN" --data-dir "$data_dir" mutate \
      --target "$TARGET" \
      --input-dir "$CORPUS_DIR_ABS" \
      --out-dir "$batch_dir" \
      --count "$MUTATE_COUNT" \
      --seed "$((block_index * 100000 + iter))" \
      "${mutate_args[@]}"
    then
      log "Arm A local block=${block_index} iter=${iter} mutate failed; skipping run"
      rm -rf "$batch_dir"
      continue
    fi

    log "Arm A local block=${block_index} iter=${iter} run"
    if ! "$TOOL_BIN" --data-dir "$data_dir" run \
      --target "$TARGET" \
      --backend local-harness \
      --workers "$WORKERS_LOCAL" \
      --timeout-sec "$TIMEOUT_SEC" \
      --restart-limit "$RESTART_LIMIT" \
      --corpus-dir "$batch_dir"
    then
      log "Arm A local block=${block_index} iter=${iter} run failed; continuing"
    fi
  done
}

run_libfuzzer_block() {
  local data_dir="$1"
  local tag="$2"
  local block_seconds="$3"
  export TOOL_LIBFUZZER_CMD="mkdir -p {artifact_dir} && LLVM_PROFILE_FILE={artifact_dir}/onnx-native-%p.profraw $WORKDIR/harnesses/libfuzzer/onnxruntime_loader_fuzzer -artifact_prefix={artifact_dir}/ -max_total_time=$LIBFUZZER_MAX_TOTAL_TIME {corpus_dir} >/dev/null 2>&1"
  DATA_DIR="$data_dir" WORKDIR="$WORKDIR" TOOL_BIN="$TOOL_BIN" \
    bash "$WORKDIR/scripts/run_long.sh" \
      --target "$TARGET" \
      --backend libfuzzer \
      --duration-seconds "$block_seconds" \
      --tag "$tag" \
      --corpus-dir "$CORPUS_DIR_ABS" \
      --workers "$WORKERS_LIBFUZZER" \
      --timeout-sec "$TIMEOUT_SEC" \
      --restart-limit "$RESTART_LIMIT"
}

run_aflpp_block() {
  local data_dir="$1"
  local tag="$2"
  local block_seconds="$3"
  local ld_path
  ld_path="{container_workdir}/data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2/build/Linux/Release:{container_workdir}/data/targets/onnxruntime/v1.23.2/build-src/onnxruntime-1.23.2/build/Linux/Release"
  export TOOL_AFLPP_CMD="docker run --rm {docker_user_flag} {docker_hardening_flags} {docker_readonly_flags} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc \"LD_LIBRARY_PATH=$ld_path:\\\$LD_LIBRARY_PATH afl-fuzz -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- {container_workdir}/harnesses/aflpp/onnxruntime_loader_replay @@ >/dev/null 2>&1\""
  DATA_DIR="$data_dir" WORKDIR="$WORKDIR" TOOL_BIN="$TOOL_BIN" \
    bash "$WORKDIR/scripts/run_long.sh" \
      --target "$TARGET" \
      --backend aflpp \
      --duration-seconds "$block_seconds" \
      --tag "$tag" \
      --corpus-dir "$CORPUS_DIR_ABS" \
      --workers "$WORKERS_AFLPP" \
      --timeout-sec "$TIMEOUT_SEC" \
      --restart-limit "$RESTART_LIMIT"
}

run_block() {
  local block_index="$1"
  local day="$2"
  local slot="$3"
  local arm="$4"
  local backend
  local workers
  local data_dir
  local tag
  local block_log
  local start_ts
  local end_ts
  local end_epoch
  local ec=0

  backend="$(arm_backend "$arm")"
  workers="$(arm_workers "$arm")"
  data_dir="$CAMPAIGN_ROOT/arm-${arm}-${backend}"
  tag="${CAMPAIGN_ID}-block$(printf '%03d' "$block_index")-day${day}-arm${arm}-${backend}"
  block_log="$CAMPAIGN_ROOT/logs/block$(printf '%03d' "$block_index")-day${day}-arm${arm}-${backend}.log"
  mkdir -p "$data_dir"
  mkdir -p "$(dirname "$block_log")"

  start_ts="$(date -Iseconds)"
  end_epoch=$(( $(date +%s) + BLOCK_DURATION_SECONDS ))
  log "block=${block_index}/${TOTAL_BLOCKS} day=${day} slot=${slot} arm=${arm} backend=${backend} workers=${workers} duration=${BLOCK_DURATION_SECONDS}s"
  write_status_json "running" "block=${block_index} arm=${arm} backend=${backend}" "$((block_index - 1))"

  set +e
  {
    case "$arm" in
      A) run_local_block "$data_dir" "$block_index" "$end_epoch" ;;
      B) run_libfuzzer_block "$data_dir" "$tag" "$BLOCK_DURATION_SECONDS" ;;
      C) run_aflpp_block "$data_dir" "$tag" "$BLOCK_DURATION_SECONDS" ;;
    esac
  } > >(tee -a "$block_log") 2>&1
  ec=$?
  set -e

  end_ts="$(date -Iseconds)"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$block_index" "$day" "$slot" "$arm" "$backend" "$workers" "$start_ts" "$end_ts" "$ec" "$data_dir" "$block_log" \
    >> "$STATUS_TSV"
  host_snapshot "$HOST_LOG"

  if [[ "$ec" -ne 0 ]]; then
    log "block=${block_index} arm=${arm} backend=${backend} exited with ec=${ec}; continuing campaign"
  else
    log "block=${block_index} arm=${arm} backend=${backend} done"
  fi
}

cd "$WORKDIR"
DATA_ROOT_ABS="$(abs_path "$DATA_ROOT")"
CORPUS_DIR_ABS="$(abs_path "$CORPUS_DIR")"
CAMPAIGN_ROOT="$DATA_ROOT_ABS/$CAMPAIGN_ID"
MANIFEST_PATH="$CAMPAIGN_ROOT/manifest.json"
STATUS_JSON="$CAMPAIGN_ROOT/status.json"
STATUS_TSV="$CAMPAIGN_ROOT/status.tsv"
HOST_LOG="$CAMPAIGN_ROOT/host-monitor.log"
TOTAL_BLOCKS=$((DAYS * 3))
if [[ -n "$BLOCK_SECONDS" ]]; then
  BLOCK_DURATION_SECONDS="$BLOCK_SECONDS"
else
  BLOCK_DURATION_SECONDS=$((BLOCK_HOURS * 3600))
fi
GIT_COMMIT="$(git_value unknown rev-parse HEAD)"
GIT_BRANCH="$(git_value unknown rev-parse --abbrev-ref HEAD)"

if [[ "$DRY_RUN" == "1" ]]; then
  log "dry-run campaign_id=$CAMPAIGN_ID days=$DAYS block_seconds=$BLOCK_DURATION_SECONDS total_blocks=$TOTAL_BLOCKS"
  block=0
  for ((day_index = 0; day_index < DAYS; day_index++)); do
    day=$((day_index + 1))
    for slot in 0 1 2; do
      block=$((block + 1))
      arm="$(arm_for_slot "$day_index" "$slot")"
      backend="$(arm_backend "$arm")"
      workers="$(arm_workers "$arm")"
      printf 'block=%03d day=%d slot=%d arm=%s backend=%s workers=%s duration=%ss\n' \
        "$block" "$day" "$slot" "$arm" "$backend" "$workers" "$BLOCK_DURATION_SECONDS"
    done
  done
  exit 0
fi

if ! ORT_SRC_RESOLVED="$(find_ort_src)"; then
  echo "[onnx-abc-week] ONNX Runtime libonnxruntime.so not found" >&2
  echo "[onnx-abc-week] expected one of:" >&2
  echo "  $WORKDIR/data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2/build/Linux/Release/libonnxruntime.so" >&2
  echo "  $WORKDIR/data/targets/onnxruntime/v1.23.2/build-src/onnxruntime-1.23.2/build/Linux/Release/libonnxruntime.so" >&2
  echo "[onnx-abc-week] set ORT_SRC if your build lives elsewhere" >&2
  exit 2
fi
SO_DIR_RESOLVED="$ORT_SRC_RESOLVED/build/Linux/Release"
export ORT_SRC="$ORT_SRC_RESOLVED"
export SO_DIR="$SO_DIR_RESOLVED"
export LD_LIBRARY_PATH="$SO_DIR_RESOLVED:${LD_LIBRARY_PATH:-}"

if [[ "$SKIP_PREFLIGHT" != "1" ]]; then
  [[ -f "$WORKDIR/Cargo.toml" ]] || { echo "[onnx-abc-week] run from repo root or set WORKDIR" >&2; exit 2; }
  [[ -x "$TOOL_BIN" ]] || { echo "[onnx-abc-week] tool binary missing: $TOOL_BIN" >&2; exit 2; }
  [[ -d "$CORPUS_DIR_ABS" ]] || { echo "[onnx-abc-week] corpus dir missing: $CORPUS_DIR_ABS" >&2; exit 2; }
  [[ -x "$WORKDIR/harnesses/libfuzzer/onnxruntime_loader_fuzzer" ]] || { echo "[onnx-abc-week] native libFuzzer harness missing; run scripts/check_onnx_native_engines.sh" >&2; exit 2; }
  [[ -x "$WORKDIR/harnesses/aflpp/onnxruntime_loader_replay" ]] || { echo "[onnx-abc-week] native AFL++ harness missing; run scripts/check_onnx_native_engines.sh --require-aflpp" >&2; exit 2; }
  command -v docker >/dev/null 2>&1 || { echo "[onnx-abc-week] docker missing for Arm C AFL++" >&2; exit 2; }
  docker image inspect aflplusplus/aflplusplus >/dev/null 2>&1 || { echo "[onnx-abc-week] AFL++ Docker image missing; run: docker pull aflplusplus/aflplusplus" >&2; exit 2; }
  free_gb="$(free_gb_for_path "$DATA_ROOT_ABS")"
  if [[ "$free_gb" -lt "$MIN_FREE_GB" ]]; then
    echo "[onnx-abc-week] free disk is low: ${free_gb}GB < ${MIN_FREE_GB}GB at $DATA_ROOT_ABS" >&2
    exit 2
  fi
fi

mkdir -p "$CAMPAIGN_ROOT"
write_manifest
printf 'block\tday\tslot\tarm\tbackend\tworkers\tstarted_at\tended_at\texit_code\tdata_dir\tlog_file\n' > "$STATUS_TSV"
host_snapshot "$HOST_LOG"
write_status_json "starting" "preparing monitor" 0

monitor_pid=""
monitor_loop "$HOST_LOG" &
monitor_pid="$!"
cleanup() {
  if [[ -n "$monitor_pid" ]]; then
    kill "$monitor_pid" >/dev/null 2>&1 || true
    wait "$monitor_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT
completed_blocks=0
trap 'log "interrupt received"; write_status_json "interrupted" "signal" "$completed_blocks"; exit 130' INT TERM

log "starting campaign=$CAMPAIGN_ID root=$CAMPAIGN_ROOT"
log "manifest=$MANIFEST_PATH"

block=0
for ((day_index = 0; day_index < DAYS; day_index++)); do
  day=$((day_index + 1))
  for slot in 0 1 2; do
    block=$((block + 1))
    arm="$(arm_for_slot "$day_index" "$slot")"
    run_block "$block" "$day" "$slot" "$arm"
    completed_blocks="$block"
    write_status_json "running" "completed block=$block" "$completed_blocks"
  done
done

write_status_json "done" "complete" "$completed_blocks"
log "done campaign=$CAMPAIGN_ID root=$CAMPAIGN_ROOT"
log "status=$STATUS_JSON"
