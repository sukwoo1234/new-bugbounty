# ONNX Backend Comparison (6h)

## Conditions
- machine: `06-211-01`
- target: `onnx`
- corpus: `seeds/onnx`
- workers: `1`
- timeout: `30`
- restart_limit: `1`
- duration: `6h`
- git_commit: `604064e`

## Results
| backend | exit_code | runs | failures | new_paths_per_hour* | new_crashes_per_hour | valid_crash_ratio | global_error_rate_5m |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `local-harness` | `0` | `4395` | `0` | `19942` | `0` | `1.0000` | `0.0000` |
| `libfuzzer` | `0` | `2628` | `0` | `439` | `0` | `1.0000` | `0.0000` |
| `aflpp` | `0` | `580` | `0` | `98` | `0` | `1.0000` | `0.0000` |

Paper caution:
- `new_paths_per_hour*` is not a true coverage-path or edge-coverage metric.
- In the current implementation, this value is derived from successful run counts recorded during `run`, so it should be interpreted as a throughput/progress proxy only.
- It is acceptable to use this column for operational throughput comparison under identical conditions, but not for claims such as "backend A discovered more coverage" or "backend B explored more unique execution paths."

## Interpretation
- Three backends completed the 6-hour run without failure (`exit_code=0`, `failures=0`).
- Under identical conditions, observed run counts were `local-harness > libfuzzer > aflpp`.
- In this experiment, `new_crashes_per_hour = 0` for all three backends. The result supports stability and throughput comparison, but not crash-finding superiority claims.

## Notes
- `new_paths_per_hour` is currently a success-based proxy metric. It should not be presented as a direct edge-coverage count.
- Metrics snapshots were copied immediately after each backend run on the fuzz host:
  - `/tmp/metrics-20260408-onnx-local-6h-06-211-01.json`
  - `/tmp/metrics-20260408-onnx-libfuzzer-6h-06-211-01.json`
  - `/tmp/metrics-20260408-onnx-aflpp-6h-06-211-01.json`
