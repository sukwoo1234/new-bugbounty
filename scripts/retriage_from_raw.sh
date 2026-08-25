#!/usr/bin/env bash
# retriage_from_raw.sh
#
# #1 버그(triage 의미 반전, 2026-04-18 수정) 이전에 생성된 summary.json을
# raw attempts[] 데이터로 재해석해서 summary-v2.json을 생성한다.
# 원본 summary.json은 보존한다.
#
# 재매핑 규칙:
#   attempts[].result:
#     "success" -> "clean"    (exit 0 = no crash)
#     "failed"  -> "crashed"  (non-zero exit = crash)
#     "timeout" -> "timeout"  (그대로)
#
# verdict 재계산 (triage.rs 새 로직과 동일):
#   timeout_count > 0                                         -> "timeout"
#   crashed_count == total && signature_consistent            -> "reproduced"
#   crashed_count == 0                                        -> "not_reproduced"
#   !signature_consistent                                     -> "flaky_stack_mismatch"
#   crashed_count <= 1                                        -> "flaky"
#   else                                                      -> "partial"
#
# 의존: jq
#
# 사용법:
#   scripts/retriage_from_raw.sh [--data-dir DIR] [--dry-run]
#
# 참조: docs/specs.md §4

set -euo pipefail

DATA_DIR="./data"
DRY_RUN=0

usage() {
  cat <<'EOF'
usage: retriage_from_raw.sh [--data-dir DIR] [--dry-run]

Options:
  --data-dir DIR   triage 루트 검색 기반 (default: ./data)
  --dry-run        summary-v2.json을 쓰지 않고 각 triage의 재계산 결과만 출력
  -h, --help       이 도움말 출력
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --data-dir) DATA_DIR="${2:-}"; shift 2 ;;
    --dry-run)  DRY_RUN=1; shift ;;
    -h|--help)  usage; exit 0 ;;
    *) echo "[retriage] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if ! command -v jq >/dev/null 2>&1; then
  echo "[retriage] 'jq' is required but not installed" >&2
  exit 2
fi

TRIAGE_ROOT="${DATA_DIR%/}/triage"
if [[ ! -d "$TRIAGE_ROOT" ]]; then
  echo "[retriage] triage root not found: $TRIAGE_ROOT" >&2
  exit 2
fi

PROCESSED=0
SKIPPED=0
declare -A VERDICT_COUNT

for summary in "$TRIAGE_ROOT"/triage-*/summary.json; do
  [[ -f "$summary" ]] || continue

  triage_dir="$(dirname "$summary")"
  v2_path="$triage_dir/summary-v2.json"

  # 이미 신규 라벨("clean"/"crashed")을 쓰는 버전은 재해석 불필요
  if jq -e '.attempts[0].result | test("^(clean|crashed)$")' "$summary" >/dev/null 2>&1; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # 라벨 재매핑
  relabeled=$(jq '
    .attempts |= (map(
      .result = (if .result == "success" then "clean"
                 elif .result == "failed" then "crashed"
                 else .result end)
    ))
  ' "$summary")

  # 카운트 재계산
  clean_count=$(echo "$relabeled"   | jq '[.attempts[] | select(.result == "clean")]   | length')
  crashed_count=$(echo "$relabeled" | jq '[.attempts[] | select(.result == "crashed")] | length')
  timeout_count=$(echo "$relabeled" | jq '[.attempts[] | select(.result == "timeout")] | length')
  total=$(echo "$relabeled"         | jq '.attempts | length')

  # signature_consistent 재계산 (첫 시그니처와 전체 일치 여부)
  sig_consistent=$(echo "$relabeled" | jq '
    (.attempts | map(.signature_top3)) as $sigs
    | if ($sigs | length) == 0 then true
      else ($sigs | all(. == $sigs[0]))
      end
  ')

  # A31: attempts[]가 비면 "모든 시도가 크래시" 조건이 공허하게 참이 되어
  # verdict=reproduced가 찍혔다. 근거 없는 크래시 주장이므로 재해석하지 않고 건너뛴다.
  if [[ "$total" -eq 0 ]]; then
    echo "[retriage] skip: no attempts to re-derive a verdict from: $summary" >&2
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # verdict 재계산 (triage.rs 새 로직과 동일)
  if [[ "$timeout_count" -gt 0 ]]; then
    verdict="timeout"
  elif [[ "$crashed_count" -eq "$total" && "$sig_consistent" == "true" ]]; then
    verdict="reproduced"
  elif [[ "$crashed_count" -eq 0 ]]; then
    verdict="not_reproduced"
  elif [[ "$sig_consistent" != "true" ]]; then
    verdict="flaky_stack_mismatch"
  elif [[ "$crashed_count" -le 1 ]]; then
    verdict="flaky"
  else
    verdict="partial"
  fi

  # 필드 업데이트 + revised_by 스탬프
  updated=$(echo "$relabeled" | jq \
    --argjson clean "$clean_count" \
    --argjson crashed "$crashed_count" \
    --argjson tout "$timeout_count" \
    --argjson sigcons "$sig_consistent" \
    --arg verdict "$verdict" \
    '
      .clean_count = $clean |
      .crashed_count = $crashed |
      .timeout_count = $tout |
      .signature_consistent = $sigcons |
      .verdict = $verdict |
      del(.success_count, .failed_count) |
      .revised_by = "retriage_from_raw.sh (2026-04-18 #1 bug recovery)"
    ')

  VERDICT_COUNT[$verdict]=$(( ${VERDICT_COUNT[$verdict]:-0} + 1 ))
  PROCESSED=$((PROCESSED + 1))

  if [[ "$DRY_RUN" -eq 1 ]]; then
    id="$(basename "$triage_dir")"
    echo "$id: verdict=$verdict clean=$clean_count crashed=$crashed_count timeout=$timeout_count sig_consistent=$sig_consistent"
  else
    echo "$updated" > "$v2_path"
  fi
done

echo ""
echo "[retriage] done"
echo "processed: $PROCESSED"
echo "skipped (already v2 labels): $SKIPPED"
if [[ ${#VERDICT_COUNT[@]} -gt 0 ]]; then
  echo "verdict distribution:"
  for v in "${!VERDICT_COUNT[@]}"; do
    echo "  $v: ${VERDICT_COUNT[$v]}"
  done
fi
if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "(dry-run: summary-v2.json not written)"
fi
