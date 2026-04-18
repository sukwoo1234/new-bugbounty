- Source host: `06-211-01`
- Long-run logs:
  - `data/longrun/run-20260410_safetensors_local_6h_06-211-01.*`
  - `data/longrun/run-20260410_safetensors_libfuzzer_6h_06-211-01.*`
  - `data/longrun/run-20260410_safetensors_aflpp_6h_06-211-01.*`
- `aflpp` run used `{docker_user_flag}` expansion and confirmed command line included `--user 1000:1000`.
- Metric caution:
  - `new_paths_per_hour` is currently computed from `run` success counts and stored as a proxy metric.
  - It should not be cited as LLVM/libFuzzer/AFL++ edge coverage, bitmap coverage, or unique path discovery.

## ⚠️ 2026-04-18 Revision Notice (#1 triage semantic bug)

이 번들은 #1 버그(triage 의미 반전) 수정 이전에 생성되었습니다. 영향:

- **신뢰 가능 지표**: `success`, `failed`, `timeout`, `retries`, `total_runs`, `new_paths_per_hour` (proxy)
- **거짓 부풀림 (재해석 필요)**: `verdict`, `valid_crash_ratio`, `reproduced_count`, `new_crashes_per_hour`, `unique_signature_count`, `report_count`

원본 `data/triage/triage-*/summary.json`의 `attempts[].signature_top3` / `result` 값은 보존되어 있어 재해석 스크립트로 정확한 수치 복구 가능. 재해석 전까지 크래시 관련 수치는 논문/제출에 그대로 사용 금지.

참조: Phase 0 bug fix commits (2026-04-18)
