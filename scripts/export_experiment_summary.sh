#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: export_experiment_summary.sh \
  --experiment-id <id> \
  --machine-label <label> \
  --target <onnx|gguf|safetensors> \
  --backend <local-harness|libfuzzer|aflpp> \
  --duration-hours <n> \
  [--corpus-dir <dir>] \
  [--workers <n>] \
  [--timeout-sec <n>] \
  [--restart-limit <n>] \
  [--metrics-file <path>] \
  [--run-status-file <path>] \
  [--notes <text>]
EOF
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "[export-experiment] required command not found: $1" >&2
    exit 2
  fi
}

EXPERIMENT_ID=""
MACHINE_LABEL=""
TARGET=""
BACKEND=""
DURATION_HOURS=""
CORPUS_DIR=""
WORKERS="1"
TIMEOUT_SEC="30"
RESTART_LIMIT="1"
METRICS_FILE="data/metrics/latest.json"
RUN_STATUS_FILE=""
NOTES_TEXT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --experiment-id) EXPERIMENT_ID="${2:-}"; shift 2 ;;
    --machine-label) MACHINE_LABEL="${2:-}"; shift 2 ;;
    --target) TARGET="${2:-}"; shift 2 ;;
    --backend) BACKEND="${2:-}"; shift 2 ;;
    --duration-hours) DURATION_HOURS="${2:-}"; shift 2 ;;
    --corpus-dir) CORPUS_DIR="${2:-}"; shift 2 ;;
    --workers) WORKERS="${2:-}"; shift 2 ;;
    --timeout-sec) TIMEOUT_SEC="${2:-}"; shift 2 ;;
    --restart-limit) RESTART_LIMIT="${2:-}"; shift 2 ;;
    --metrics-file) METRICS_FILE="${2:-}"; shift 2 ;;
    --run-status-file) RUN_STATUS_FILE="${2:-}"; shift 2 ;;
    --notes) NOTES_TEXT="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[export-experiment] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ -z "$EXPERIMENT_ID" || -z "$MACHINE_LABEL" || -z "$TARGET" || -z "$BACKEND" || -z "$DURATION_HOURS" ]]; then
  echo "[export-experiment] required args missing" >&2
  usage
  exit 2
fi

if [[ -z "$CORPUS_DIR" ]]; then
  CORPUS_DIR="seeds/${TARGET}"
fi

need_cmd jq
need_cmd git

if [[ ! -f "$METRICS_FILE" ]]; then
  echo "[export-experiment] metrics file not found: $METRICS_FILE" >&2
  exit 2
fi

if [[ -z "$RUN_STATUS_FILE" ]]; then
  RUN_STATUS_FILE="$(ls -1t data/runs/run-*/status.json 2>/dev/null | head -n 1 || true)"
fi
if [[ -z "$RUN_STATUS_FILE" || ! -f "$RUN_STATUS_FILE" ]]; then
  echo "[export-experiment] run status file not found" >&2
  exit 2
fi

OUT_DIR="results/experiments/${EXPERIMENT_ID}"
mkdir -p "$OUT_DIR"

cp "$RUN_STATUS_FILE" "$OUT_DIR/run-status.json"
cp "$METRICS_FILE" "$OUT_DIR/metrics-latest.json"

SEED_COUNT=0
if [[ -d "$CORPUS_DIR" ]]; then
  SEED_COUNT="$(find "$CORPUS_DIR" -maxdepth 1 -type f | wc -l | tr -d ' ')"
fi

TOTAL_RUNS="$(jq -r '.total // 0' "$OUT_DIR/run-status.json")"
SUCCESS="$(jq -r '.success // 0' "$OUT_DIR/run-status.json")"
FAILED="$(jq -r '.failed // 0' "$OUT_DIR/run-status.json")"
TIMEOUT="$(jq -r '.timeout // 0' "$OUT_DIR/run-status.json")"
RETRIES="$(jq -r '.retries // 0' "$OUT_DIR/run-status.json")"

NEW_CRASHES_PER_HOUR="$(jq -r '.metrics.new_crashes_per_hour // 0' "$OUT_DIR/metrics-latest.json")"
VALID_CRASH_RATIO="$(jq -r 'if (.metrics.valid_crash_ratio_status // "legacy_unverified") == "available" then (.metrics.valid_crash_ratio // "not_available") else (.metrics.valid_crash_ratio_status // "legacy_unverified") end' "$OUT_DIR/metrics-latest.json")"
VALID_CRASH_RATIO_SOURCE="$(jq -r '.metrics.valid_crash_ratio_source // "legacy_event_log"' "$OUT_DIR/metrics-latest.json")"
SUCCESSFUL_RUNS_PER_HOUR_PROXY="$(jq -r '.metrics.successful_runs_per_hour_proxy // .metrics.new_paths_per_hour // 0' "$OUT_DIR/metrics-latest.json")"
GLOBAL_ERROR_RATE_5M="$(jq -r '.metrics.global_error_rate_5m // 0' "$OUT_DIR/metrics-latest.json")"

TRIAGE_INDEX="$OUT_DIR/triage-index.tsv"
{
  echo -e "triage_id\tverdict\tinput_path\tsignature_top1\tsummary_path"
  for f in $(ls -1 data/triage/triage-*/summary.json 2>/dev/null | sort); do
    triage_id="$(jq -r '.triage_id // ""' "$f")"
    verdict="$(jq -r '.verdict // ""' "$f")"
    input_path="$(jq -r '.input // ""' "$f")"
    sig="$(jq -r '.attempts[0].signature_top3[0] // ""' "$f")"
    echo -e "${triage_id}\t${verdict}\t${input_path}\t${sig}\t${f}"
  done
} > "$TRIAGE_INDEX"

REPORT_INDEX="$OUT_DIR/report-index.tsv"
{
  echo -e "report_id\tsource_triage_id\tsuggested_severity\tseverity_confidence\treport_path\tmeta_path"
  for f in $(ls -1 data/reports/report-*/meta.json 2>/dev/null | sort); do
    report_id="$(jq -r '.report_id // ""' "$f")"
    source_triage_id="$(jq -r '.source_triage_id // ""' "$f")"
    suggested_severity="$(jq -r '.suggested_severity // ""' "$f")"
    severity_confidence="$(jq -r '.severity_confidence // ""' "$f")"
    report_path="$(dirname "$f")/report.md"
    echo -e "${report_id}\t${source_triage_id}\t${suggested_severity}\t${severity_confidence}\t${report_path}\t${f}"
  done
} > "$REPORT_INDEX"

REPRODUCED_COUNT="$(awk -F'\t' 'NR>1 && $2=="reproduced" {c++} END{print c+0}' "$TRIAGE_INDEX")"
REPORT_COUNT="$(awk 'END{print NR-1}' "$REPORT_INDEX")"
UNIQUE_SIGNATURE_COUNT="$(awk -F'\t' 'NR>1 && $4!="" {print $4}' "$TRIAGE_INDEX" | sort -u | wc -l | tr -d ' ')"

GIT_COMMIT="$(git rev-parse --short HEAD)"
STARTED_AT="$(date -Iseconds)"
FINISHED_AT="$(date -Iseconds)"

cat > "$OUT_DIR/manifest.json" <<EOF
{
  "experiment_id": "${EXPERIMENT_ID}",
  "machine_label": "${MACHINE_LABEL}",
  "target": "${TARGET}",
  "backend": "${BACKEND}",
  "workers": ${WORKERS},
  "timeout_sec": ${TIMEOUT_SEC},
  "restart_limit": ${RESTART_LIMIT},
  "duration_hours": ${DURATION_HOURS},
  "corpus_dir": "${CORPUS_DIR}",
  "seed_count": ${SEED_COUNT},
  "git_commit": "${GIT_COMMIT}",
  "started_at": "${STARTED_AT}",
  "finished_at": "${FINISHED_AT}",
  "metrics_file": "${METRICS_FILE}",
  "run_status_file": "${RUN_STATUS_FILE}"
}
EOF

cat > "$OUT_DIR/summary.md" <<EOF
# Experiment Summary

## Conditions
- experiment_id: \`${EXPERIMENT_ID}\`
- machine: \`${MACHINE_LABEL}\`
- target: \`${TARGET}\`
- backend: \`${BACKEND}\`
- duration_hours: \`${DURATION_HOURS}\`
- corpus_dir: \`${CORPUS_DIR}\`
- seed_count: \`${SEED_COUNT}\`
- workers: \`${WORKERS}\`
- timeout_sec: \`${TIMEOUT_SEC}\`
- restart_limit: \`${RESTART_LIMIT}\`
- git_commit: \`${GIT_COMMIT}\`

## Metrics
| key | value |
| --- | ---: |
| total_runs | ${TOTAL_RUNS} |
| success | ${SUCCESS} |
| failed | ${FAILED} |
| timeout | ${TIMEOUT} |
| retries | ${RETRIES} |
| new_crashes_per_hour | ${NEW_CRASHES_PER_HOUR} |
| valid_crash_ratio | ${VALID_CRASH_RATIO} |
| valid_crash_ratio_source | ${VALID_CRASH_RATIO_SOURCE} |
| reproduced_count | ${REPRODUCED_COUNT} |
| report_count | ${REPORT_COUNT} |
| unique_signature_count | ${UNIQUE_SIGNATURE_COUNT} |
| successful_runs_per_hour_proxy | ${SUCCESSFUL_RUNS_PER_HOUR_PROXY} |
| global_error_rate_5m | ${GLOBAL_ERROR_RATE_5M} |

## Caveat
- \`successful_runs_per_hour_proxy\` is a success-count proxy metric, not true edge/path coverage.
- \`new_crashes_per_hour\` is based on triage inputs where a crash was observed.
- \`global_error_rate_5m\` is computed as recent \`errors / total\` over metric events.
- \`valid_crash_ratio\` is calculated from \`data/triage/triage-*/summary.json\` when \`valid_crash_ratio_source=triage_summary_scan\`.
- \`valid_crash_ratio\` is \`not_available\` when there are no triage crash observations to support the ratio.
- Legacy metrics without \`valid_crash_ratio_status\` are reported as \`legacy_unverified\`.
EOF

{
  echo "- generated_by: scripts/export_experiment_summary.sh"
  echo "- experiment_id: \`${EXPERIMENT_ID}\`"
  if [[ -n "$NOTES_TEXT" ]]; then
    echo "- note: ${NOTES_TEXT}"
  fi
} > "$OUT_DIR/notes.md"

echo "[export-experiment] done"
echo "out_dir: $OUT_DIR"
echo "files:"
echo "  - $OUT_DIR/manifest.json"
echo "  - $OUT_DIR/summary.md"
echo "  - $OUT_DIR/run-status.json"
echo "  - $OUT_DIR/metrics-latest.json"
echo "  - $OUT_DIR/triage-index.tsv"
echo "  - $OUT_DIR/report-index.tsv"
echo "  - $OUT_DIR/notes.md"
