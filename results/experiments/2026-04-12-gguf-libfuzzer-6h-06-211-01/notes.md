- generated_by: scripts/export_experiment_summary.sh
- experiment_id: `2026-04-12-gguf-libfuzzer-6h-06-211-01`

## ⚠️ 2026-04-18 Revision Notice (#1 triage semantic bug)

이 번들은 #1 버그(triage 의미 반전) 수정 이전에 생성되었습니다. 영향:

- **신뢰 가능 지표**: `success`, `failed`, `timeout`, `retries`, `total_runs`, `new_paths_per_hour` (proxy)
- **거짓 부풀림 (재해석 필요)**: `verdict`, `valid_crash_ratio`, `reproduced_count`, `new_crashes_per_hour`, `unique_signature_count`, `report_count`

원본 `data/triage/triage-*/summary.json`의 `attempts[].signature_top3` / `result` 값은 보존되어 있어 재해석 스크립트로 정확한 수치 복구 가능. 재해석 전까지 크래시 관련 수치는 논문/제출에 그대로 사용 금지.

참조: Phase 0 bug fix commits (2026-04-18)
