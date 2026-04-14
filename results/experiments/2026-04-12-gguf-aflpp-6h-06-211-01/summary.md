# Experiment Summary

## Conditions
- experiment_id: `2026-04-12-gguf-aflpp-6h-06-211-01`
- machine: `06-211-01`
- target: `gguf`
- backend: `aflpp`
- duration_hours: `6`
- corpus_dir: `seeds/gguf`
- seed_count: `613`
- workers: `1`
- timeout_sec: `30`
- restart_limit: `1`
- git_commit: `570020f`

## Metrics
| key | value |
| --- | ---: |
| total_runs | 1 |
| success | 1 |
| failed | 0 |
| timeout | 0 |
| retries | 0 |
| new_crashes_per_hour | 0 |
| valid_crash_ratio | 1.0000 |
| reproduced_count | 1 |
| report_count | 2 |
| unique_signature_count | 1 |
| new_paths_per_hour* | 185 |
| global_error_rate_5m | 0.0000 |

## Caveat
- `new_paths_per_hour` is a success-based proxy metric in current implementation.
- Do not present it as true edge/path coverage.
