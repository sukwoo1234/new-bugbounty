# 진행 로그 (1.0)

이 문서는 세션 중 진행한 작업을 `태스크 / 완료 기준 / 결과 / 검증` 형식으로 기록한다.

## Phase 19: Crash Artifact 관리 정책 확정

### 태스크
- 대량 실험 결과(`run/triage/report`) 누적 대비 운영 정책 문서화

### 완료 기준
- 중복/보관/압축/정리 원칙이 공개 운영 문서에 명시
- TODO의 crash artifact 정책 항목 완료 처리

### 결과
- `docs/experiment-ops.md`에 `Crash Artifact 관리 정책` 섹션 추가
  - 중복 시그니처 정리 기준
  - 보관 상한/용량 점검 기준
  - retention 기반 압축/정리 연계
  - 정리 전 `results/experiments` export 우선 원칙
- `docs/dev-todo-legacy-upgrade.md`의 crash artifact 항목 완료 처리

### 검증
- 문서 기준 점검 명령 추가:
  - `find data/runs -type f -name '*.log' | wc -l`
  - `find data/triage -maxdepth 1 -type d -name 'triage-*' | wc -l`
  - `find data/reports -maxdepth 1 -type d -name 'report-*' | wc -l`
  - `du -sh data/runs data/triage data/reports`

## Phase 18: Discord 공통 notifier + 비교 지표 템플릿 확정

### 태스크
- `run_backend_loop.sh` 공통 Discord notifier 통합
- Exporter 기반 비교 지표 템플릿 고정 문서화

### 완료 기준
- 장시간 루프에서 START/DONE/FAIL + run 결과 기반 ALERT 전송 경로 구현
- `docs/experiment-ops.md`에 notifier 설정/고정 지표 컬럼 반영

### 결과
- `scripts/run_backend_loop.sh`
  - `DISCORD_WEBHOOK`/`HOOK_FILE` 기반 webhook 로딩 추가
  - START/DONE/FAIL 알림 추가
  - 최신 `status.json`의 `failed/timeout` 감지 시 ALERT 알림 추가
- `docs/experiment-ops.md`
  - Discord 공통 설정 섹션 추가
  - 비교 지표 템플릿(고정 컬럼 + proxy metric 주의) 추가
- `docs/dev-todo-legacy-upgrade.md`
  - notifier 항목 완료 처리
  - 비교 지표 수집 템플릿 항목 완료 처리

### 검증
- 정적 검증:
  - `bash -n scripts/run_backend_loop.sh`
  - notifier 미설정 시 기존 동작 영향 없음(무시)
  - notifier 설정 시 longrun 로그와 Discord 메시지 기준으로 START/DONE/FAIL/ALERT 확인 가능

## Phase 17: G-3-2 Target Build UI/API 완료 검증 (06-211-01)

### 태스크
- G-3-2(Target upload/build UI/API) 최종 검증 및 완료 처리

### 완료 기준
- `gguf/onnx/safetensors` build 산출물 확인
- UI route 검증에서 `/target/status`, `/target/build/status` 포함 통과

### 결과
- `gguf`: `llama-cli` 산출물 확인 완료
- `onnx`: `libonnxruntime.so.1.23.2` 산출물 확인 완료
  - 참고: `expected artifact missing: libonnxruntime.so` 메시지는 비버전 파일명 기대치 차이(버전 파일은 생성됨)
- `safetensors`: `libsafetensors-*.rlib` 산출물 확인 완료
- UI/API: `scripts/check_ui_routes.sh`에서 `/target/status`, `/target/build/status` 포함 `[OK]` 통과

### 검증
- 사용자 실환경 명령:
  - `./target/debug/tool prepare-target --target gguf|onnx|safetensors`
  - `bash scripts/build_prepared_target.sh ./data gguf|onnx|safetensors`
  - `scripts/check_ui_routes.sh`

## Phase 16: ONNX 중단/복구 시나리오 검증 완료 (06-211-01)

### 태스크
- 장시간 루프 중 명시적 중단(`Ctrl+C`) 후 동일 TAG 재실행 복구 검증

### 완료 기준
- 동일 TAG 재실행 후 `DONE`/`exit=0` 기록
- `.exit` 파일/metrics 갱신 정상 확인

### 결과
- 대상: `onnx`, `local-harness`
- TAG: `20260414_onnx_local_recovery_1h_06-211-01`
- 2회 중단 후 동일 TAG 재실행에서 최종 완료:
  - `exit=0`, `runs=748`, `failures=0`

### 검증
- 사용자 실환경 확인:
  - `tail -n 40 data/longrun/run-20260414_onnx_local_recovery_1h_06-211-01.log`
  - `cat data/longrun/run-20260414_onnx_local_recovery_1h_06-211-01.exit`
  - `cat data/metrics/latest.json`

### 주의
- `new_paths_per_hour`는 현재 proxy metric이며 true coverage 지표로 해석하면 안 됨

## Phase 15: GGUF 6h backend 비교 검증 완료 (06-211-01)

### 태스크
- GGUF target 기준 `local-harness/libfuzzer/aflpp` 6시간 운영 검증

### 완료 기준
- 3개 backend 모두 `exit=0`
- longrun 로그/exit 파일 및 metrics snapshot 확보

### 결과
- `20260412_gguf_local_6h_06-211-01`: `exit=0`, `runs=8826`, `failures=0`
- `20260412_gguf_libfuzzer_6h_06-211-01`: `exit=0`, `runs=2656`, `failures=0`
- `20260412_gguf_aflpp_6h_06-211-01`: `exit=0`, `runs=1107`, `failures=0`
- metrics snapshot:
  - `/tmp/metrics-20260412_gguf_local_6h_06-211-01.json`
  - `/tmp/metrics-20260412_gguf_libfuzzer_6h_06-211-01.json`
  - `/tmp/metrics-20260412_gguf_aflpp_6h_06-211-01.json`

### 검증
- 사용자 실환경 확인:
  - `tail -n 40 data/longrun/run-20260412_gguf_*_6h_06-211-01.log`
  - `cat data/longrun/run-20260412_gguf_*_6h_06-211-01.exit`
  - `cat data/metrics/latest.json`

### 주의
- `new_paths_per_hour`는 현재 proxy metric이며 true coverage 지표로 해석하면 안 됨

## Phase 14: GGUF 1h backend 비교 검증 완료 (06-211-01)

### 태스크
- GGUF target 기준 `local-harness/libfuzzer/aflpp` 1시간 운영 검증

### 완료 기준
- 3개 backend 모두 `exit=0`
- longrun 로그/exit 파일 및 metrics snapshot 확보

### 결과
- `20260412_gguf_local_1h_06-211-01`: `exit=0`, `runs=1484`, `failures=0`
- `20260412_gguf_libfuzzer_1h_06-211-01`: `exit=0`, `runs=445`, `failures=0`
- `20260412_gguf_aflpp_1h_06-211-01`: `exit=0`, `runs=194`, `failures=0`
- metrics snapshot:
  - `/tmp/metrics-20260412_gguf_local_1h_06-211-01.json`
  - `/tmp/metrics-20260412_gguf_libfuzzer_1h_06-211-01.json`
  - `/tmp/metrics-20260412_gguf_aflpp_1h_06-211-01.json`

### 검증
- 사용자 실환경 확인:
  - `tail -n 40 data/longrun/run-20260412_gguf_*_1h_06-211-01.log`
  - `cat data/longrun/run-20260412_gguf_*_1h_06-211-01.exit`
  - `cat data/metrics/latest.json`

### 주의
- `new_paths_per_hour`는 현재 proxy metric이며 true coverage 지표로 해석하면 안 됨

## Phase 13: GGUF harness `>` 반복 출력 이슈 수정

### 태스크
- GGUF harness 실행 시 `>`가 반복 출력되며 종료되지 않는 문제 원인 분석/수정

### 완료 기준
- `tool harness --target gguf`가 대화형 루프 없이 종료
- 퍼징 머신(`06-211-01`)에서 동일 명령 재검증 성공

### 결과
- 원인: `gguf` 라이브러리 연결 probe가 `llama-cli`를 호출하며 일부 GGUF에서 REPL 모드로 진입
- 수정: `src/main.rs`의 `gguf_library_connect()`를 비대화형 probe(`llama-gguf-hash --sha256`) 기반으로 변경
- 커밋: `570020f` (`fix: avoid gguf harness REPL loop by using non-interactive parser probe`)

### 검증
- 개발 컴:
  - `cargo build --offline`
  - `./target/debug/tool harness --target gguf --input seeds/gguf/stories260K-f32.gguf` 성공
- 퍼징 머신(`06-211-01`):
  - `git pull` -> `cargo build --offline` -> 동일 harness 명령 성공
  - 출력: `library_step: llama.cpp parser connected (.../llama-gguf-hash: sha256 ...)`

## Phase 12: seed fetch 자동화 + safetensors 6h 완료

### 태스크
- safetensors 6h 3백엔드 검증 완료
- seed 다운로드/정리 자동화 스크립트 추가

### 완료 기준
- safetensors `local-harness/libfuzzer/aflpp` 6h 완료값 확보
- seed fetch 스크립트가 URL 검증/sha256/추출/sync 경로를 제공

### 결과
- safetensors 6h 완료:
  - `20260410_safetensors_local_6h_06-211-01`: `exit=0`, `runs=763`, `failures=0`
  - `20260410_safetensors_libfuzzer_6h_06-211-01`: `exit=0`, `runs=743`, `failures=0`
  - `20260410_safetensors_aflpp_6h_06-211-01`: `exit=0`, `runs=5804`, `failures=0`
- 결과 번들 추가:
  - `results/experiments/2026-04-10-safetensors-backend-comparison-6h-06-211-01/{summary.md,manifest.json,notes.md}`
- 운영 자동화 스크립트 추가:
  - `scripts/run_long.sh`
  - `scripts/collect_longrun.sh`
  - `scripts/export_experiment_summary.sh`
  - `scripts/seed_fetch.sh`

### 검증
- 사용자 실환경 longrun 결과 확인:
  - `data/longrun/run-20260410_safetensors_aflpp_6h_06-211-01.exit`
- metrics snapshot 보존:
  - `/tmp/metrics-20260410_safetensors_aflpp_6h_06-211-01.json`
- 스크립트 문법/도움말 확인:
  - `bash -n scripts/run_long.sh scripts/collect_longrun.sh scripts/export_experiment_summary.sh scripts/seed_fetch.sh`
  - `bash scripts/export_experiment_summary.sh --help`

## Phase 11: safetensors 장시간 검증 착수 + 운영 자동화 계획

### 태스크
- safetensors 백엔드 검증(1h/6h) 진행
- CLI 운영 간소화 및 Exporter v1 구현 계획 문서화

### 완료 기준
- safetensors 1h 검증 3백엔드(`local-harness/libfuzzer/aflpp`) 완료
- safetensors 6h 검증 3백엔드 완료
- 운영 자동화 계획 문서(`docs/plans/*`) 생성
- 운영 자동화 스크립트(v1) 구현
- safetensors 6h 비교 결과 번들 생성

### 결과
- safetensors 1h 검증 완료:
  - `20260410_safetensors_local_1h_06-211-01`: `exit=0`, `runs=128`, `failures=0`
  - `20260410_safetensors_libfuzzer_1h_06-211-01`: `exit=0`, `runs=125`, `failures=0`
  - `20260410_safetensors_aflpp_1h_06-211-01`: `exit=0`, `runs=971`, `failures=0`
- safetensors 6h 진행:
  - `20260410_safetensors_local_6h_06-211-01`: `exit=0`, `runs=763`, `failures=0`
  - `20260410_safetensors_libfuzzer_6h_06-211-01`: `exit=0`, `runs=743`, `failures=0`
  - `20260410_safetensors_aflpp_6h_06-211-01`: `exit=0`, `runs=5804`, `failures=0`
- 계획 문서 추가:
  - `docs/plans/bugbounty-exporter-v1-plan.md`
  - `docs/plans/cli-ops-simplification-plan.md`
- 운영 자동화 스크립트(v1) 추가:
  - `scripts/run_long.sh`
  - `scripts/collect_longrun.sh`
  - `scripts/export_experiment_summary.sh`
- safetensors 6h 결과 번들 추가:
  - `results/experiments/2026-04-10-safetensors-backend-comparison-6h-06-211-01/summary.md`
  - `results/experiments/2026-04-10-safetensors-backend-comparison-6h-06-211-01/manifest.json`
  - `results/experiments/2026-04-10-safetensors-backend-comparison-6h-06-211-01/notes.md`

### 검증
- 사용자 실환경 longrun 결과 확인:
  - `data/longrun/run-20260410_safetensors_*_1h_06-211-01.exit`
  - `data/longrun/run-20260410_safetensors_local_6h_06-211-01.exit`
  - `data/longrun/run-20260410_safetensors_libfuzzer_6h_06-211-01.exit`
- metrics snapshot 보존:
  - `/tmp/metrics-20260410_safetensors_local_1h_06-211-01.json`
  - `/tmp/metrics-20260410_safetensors_libfuzzer_1h_06-211-01.json`
  - `/tmp/metrics-20260410_safetensors_aflpp_1h_06-211-01.json`
  - `/tmp/metrics-20260410_safetensors_local_6h_06-211-01.json`
  - `/tmp/metrics-20260410_safetensors_libfuzzer_6h_06-211-01.json`
  - `/tmp/metrics-20260410_safetensors_aflpp_6h_06-211-01.json`

## Phase 1: 스캐폴딩

### 태스크
- 기본 설정/경로 로딩 (`./data`, `./seeds`)

### 완료 기준
- 실행 시 기본 디렉터리 구조 자동 생성
- 경로 충돌(파일 vs 디렉터리) 시 명확한 오류 반환

### 결과
- `AppPaths::prepare` 추가
- `data/queue/{pending,processing,done,failed,quarantine,quarantine/broken}` + `data/artifacts` 생성 로직 추가
- `seeds` 디렉터리 생성 로직 추가

### 검증
- `cargo build --offline` 성공
- `cargo run --offline -- run` 성공

## Phase 2: 타깃 다운로드/메타

### 태스크
- 공식 배포본 다운로드 규칙 적용
- 버전 고정 + 해시 기록
- 메타 저장 포맷 정의

### 완료 기준
- 타깃별 기본 버전/URL 제공
- URL 정책 검증(https + 공식 호스트/경로)
- 다운로드 후 SHA-256 계산 및 `meta.json` 저장

### 결과
- `prepare-target` 명령 추가
- 타깃 프리셋(gguf/onnx/safetensors) 추가
- `curl`/`wget` 다운로드, `sha256sum`/`shasum` 해시 계산 fallback 추가
- `data/targets/<target>/<version>/meta.json` 저장

### 검증
- `cargo build --offline` 성공
- URL 정책 위반 입력 시 실패 메시지 확인

### 제한 사항
- Codex 샌드박스에서는 DNS/네트워크 제한이 있어 원격 다운로드 검증이 어려울 수 있음
- 사용자 WSL 환경에서는 네트워크 정상 확인(`curl`, `nslookup`)

## Phase 3: 하네스 통합 (1차)

### 태스크
- 공통 하네스 라우팅
- 포맷별 프리체크 하네스
- 실제 라이브러리 연결을 위한 외부 실행 훅

### 완료 기준
- `tool harness` 명령으로 포맷별 입력 점검 가능
- 성공/실패 사유를 구조화된 텍스트로 출력

### 결과
- `harness` 명령 추가
- GGUF/ONNX/safetensors 프리체크 구현
- 외부 하네스 훅 환경변수 추가
  - `TOOL_GGUF_HARNESS_CMD`
  - `TOOL_ONNX_HARNESS_CMD`
  - `TOOL_SAFETENSORS_HARNESS_CMD`

### 검증
- `cargo build --offline` 성공
- 샘플 입력 3종으로 `tool harness` 성공 확인

### 남은 작업
- `llama.cpp / onnxruntime / safetensors` 실제 라이브러리 직접 연결

## Phase 3: 하네스 통합 (잔여작업 재시도)

### 태스크
- 실제 라이브러리 직접 연결 재시도
- 실패 원인을 실행 결과로 명확히 출력

### 완료 기준
- `tool harness` 실행 시 `direct_step` 항목에서 라이브러리 직접 호출 결과를 표시
- 모듈/바이너리 미설치 시 스킵 사유를 구체적으로 출력

### 결과
- ONNX: `python3 + onnxruntime` 직접 probe 추가
- safetensors: `python3 + safetensors` 직접 probe 추가
- GGUF: `llama-cli` 직접 probe 추가
- 보고 출력에 `direct_step` 필드 추가

### 검증
- `cargo build --offline` 성공
- 초기 검증에서 `direct_step` 미설치 사유 출력 확인

### 제한 사항
- 초기에는 미설치 상태로 인해 직접 연결이 "코드 경로 준비 + 미설치 감지" 단계였음

## Phase 3: 하네스 통합 (경로 자동 탐지 보강)

### 태스크
- 설치된 로컬 경로를 자동 탐지해 직접 연결 성공률 개선

### 완료 기준
- `.venv` 파이썬 우선 사용
- 프로젝트 내부 `llama-cli` 경로 자동 탐지

### 결과
- Python probe에 `TOOL_PYTHON_BIN` + `.venv/bin/python3` fallback 추가
- GGUF probe에 `TOOL_LLAMA_CLI_BIN` + `tools/llama.cpp/build/bin/llama-cli` fallback 추가

### 검증
- `cargo build --offline` 통과

## Phase 3: 하네스 통합 (WSL 설치 후 재검증)

### 태스크
- 실제 설치된 로컬 라이브러리 기준으로 direct probe 재검증

### 완료 기준
- `direct_step`이 미설치가 아닌 "실제 라이브러리 로더 실행 결과"를 반환

### 결과
- ONNX: `onnxruntime` 모듈 로드 후 모델 파싱 경로 실행 확인
- safetensors: `safetensors` 모듈 로드 후 헤더 파싱 경로 실행 확인
- GGUF: `llama-cli` 실행 경로 확인(샘플 입력 파싱 실패는 데이터 품질 이슈로 분리)

### 검증
- 사용자 WSL 실행 결과 기준:
  - ONNX: `ModelProto does not have a graph` (로더 실행됨)
  - safetensors: `missing field 'shape'` (로더 실행됨)
  - GGUF: `failed to read key-value pairs` (llama-cli 실행됨)

## 다음 우선순위
1. 재현/검증 파이프라인(Phase 5)
2. 리포트/보관 자동화(Phase 6)
3. 운영 지표 수집(Phase 7)

## Phase 4: 퍼저 실행 파이프라인

### 태스크
- `tool run`을 실제 파이프라인으로 전환
- 병렬 실행 기본값 8 적용
- 타임아웃/재시도(재시작) 정책 반영

### 완료 기준
- 타깃/코퍼스 기준으로 하네스 작업을 큐잉하고 병렬 실행
- 입력별 실행 로그 저장
- 실행 결과 요약(`success/failed/timeout/retries`) 저장

### 결과
- `run` 명령 인자 추가: `--target`, `--corpus-dir`, `--workers`, `--timeout-sec`, `--restart-limit`, `--max-jobs`
- 워커 스레드 기반 작업 큐(`VecDeque`) 병렬 처리 구현
- 서브프로세스 실행 시 스레드 억제 환경 변수 적용
- `timeout` 명령이 있으면 per-input 타임아웃 적용
- 재시도 정책: 실패/타임아웃 시 `restart_limit`만큼 재실행
- 로그 저장: `data/runs/run-<unix>/logs/job-*.log`
- 상태 저장: `data/runs/run-<unix>/status.json`

### 검증
- `cargo build --offline` 통과
- `tool run --target onnx --corpus-dir /tmp/bugbounty-corpus --workers 8 --timeout-sec 5 --restart-limit 1 --max-jobs 2` 스모크 실행 성공

## Phase 5: 재현/검증 파이프라인

### 태스크
- `tool triage`를 3회 재현 검증 파이프라인으로 구현
- 시그니처 top3 비교 로직 추가
- 실패 모드 분기(`flaky`, `timeout`) 반영

### 완료 기준
- 입력 1건에 대해 반복 실행(기본 3회) 수행
- 시도별 시그니처 top3를 수집하고 일관성 판정
- 결과 요약을 파일로 저장

### 결과
- `triage` 명령 인자 추가: `--target`, `--input`, `--repro-retries`, `--timeout-sec`
- 시도별 로그 저장: `data/triage/triage-<unix>/attempt-<n>.log`
- 요약 저장: `data/triage/triage-<unix>/summary.json`
- 판정 로직: `reproduced`, `flaky`, `flaky_stack_mismatch`, `timeout`, `failed`

### 검증
- `cargo build --offline` 통과
- `tool triage --target onnx --input /tmp/bugbounty-harness-samples/sample.onnx --repro-retries 3 --timeout-sec 10` 실행 성공
- `summary.json`에 시도별 signature_top3 및 verdict 기록 확인

## Phase 6: 리포트/보관 파이프라인

### 태스크
- `tool report` 자동 생성 구현
- 보관 정책 적용(30일, 로그 zstd, core dump OFF)

### 완료 기준
- 최신 triage 결과를 기준으로 report/evidence 파일 자동 생성
- 30일 초과 로그 압축(zstd) 및 30일 초과 run/triage/report 디렉터리 정리
- 하네스/triage/direct probe 외부 실행 시 core dump 비활성화 기본 적용

### 결과
- `report` 명령을 stub에서 파이프라인으로 전환
- 최신 `data/triage/triage-*/summary.json` 자동 탐색 후 산출물 생성
  - `data/reports/report-<unix>/report.md`
  - `data/reports/report-<unix>/crash_report.txt`
  - `data/reports/report-<unix>/repro.sh`
  - `data/reports/report-<unix>/meta.json`
- 보관 정책 함수 추가
  - 30일 초과 `.log` 파일 zstd 압축(`--rm`)
  - 30일 초과 `run-*`, `triage-*`, `report-*` 디렉터리 삭제
  - `zstd` 미설치 시 skip 카운트 기록
- core dump OFF 기본 정책 추가
  - 가능 시 `prlimit --core=0 -- <cmd>` 래핑
  - `ASAN_OPTIONS`에 `disable_coredump=1` 강제 포함

### 검증
- `cargo build --offline` 통과
- `tool report` 실행 성공
- `data/reports/report-*/` 경로에 `report.md`, `crash_report.txt`, `repro.sh`, `meta.json` 생성 확인

## Phase 7: 운영 지표 수집 정의

### 태스크
- run/triage 결과를 기반으로 운영 지표를 파일로 수집
- 1시간/5분 윈도우 기준 집계 스냅샷 생성

### 완료 기준
- 이벤트 누적 파일과 최신 지표 스냅샷 파일 생성
- 최소 지표(신규 경로/신규 크래시/유효율/5분 에러율) 계산

### 결과
- 지표 이벤트 append 로직 추가: `data/metrics/events.jsonl`
  - `run` 이벤트: `total`, `errors`, `new_paths(proxy=success)`
  - `triage` 이벤트: `total`, `errors`, `new_crashes`, `valid_crashes`
- 스냅샷 생성 로직 추가: `data/metrics/latest.json`
  - `new_paths_per_hour`
  - `new_crashes_per_hour`
  - `valid_crash_ratio`
  - `global_error_rate_5m`
- 숫자 필드 파서가 공백 없는 JSONL 형식도 처리하도록 보강

### 검증
- `cargo build --offline` 통과
- `tool run --target onnx --corpus-dir /tmp/bugbounty-corpus --workers 1 --timeout-sec 5 --restart-limit 0 --max-jobs 1` 실행
- `tool triage --target onnx --input /tmp/bugbounty-corpus/min.onnx --repro-retries 2 --timeout-sec 5` 실행
- `data/metrics/latest.json` 생성 및 지표 값 갱신 확인

## Refactor: 기능 모듈 분리

### 태스크
- 코드 구조 규칙을 기능 기준 모듈 분리로 고정
- Phase 6/7 로직을 `main.rs`에서 모듈로 분리

### 완료 기준
- `report/retention/metrics` 기능이 각각 독립 파일로 이동
- `main.rs`는 엔트리 + 오케스트레이션 중심으로 축소
- 기존 run/triage/report 동작 동일성 유지

### 결과
- 규칙 문서 갱신: `docs/rules.md`에 코드 구조 규칙 추가
- 신규 모듈 파일 추가:
  - `src/report.rs`
  - `src/retention.rs`
  - `src/metrics.rs`
- `src/main.rs`는 모듈 선언 + 위임 호출 중심으로 정리

### 검증
- `cargo build --offline` 통과
- `tool run --target onnx --corpus-dir /tmp/bugbounty-corpus --workers 1 --timeout-sec 5 --restart-limit 0 --max-jobs 1` 성공
- `tool triage --target onnx --input /tmp/bugbounty-corpus/min.onnx --repro-retries 2 --timeout-sec 5` 성공
- `tool report` 성공 및 산출물 생성 확인

## Phase 3: 하네스 통합 (라이브러리 연결 마무리)

### 태스크
- GGUF/ONNX/safetensors 하네스의 라이브러리 연결 단계를 core path로 고정

### 완료 기준
- `tool harness` 출력에 라이브러리 연결 단계가 명시되고 타깃별 연결 경로가 실행됨
- 필요 시 strict 모드(`TOOL_REQUIRE_LIBRARY_CONNECT=1`)로 미연결을 실패 처리 가능

### 결과
- `direct_step`를 `library_step`으로 전환하고 core path 문구를 실제 라이브러리 경로로 변경
  - GGUF: `llama.cpp parser`
  - ONNX: `onnxruntime session loader`
  - safetensors: `safetensors safe_open`
- strict 옵션 추가: `TOOL_REQUIRE_LIBRARY_CONNECT=1`
- GGUF 연결 판정 보강: `prlimit` 래핑 시 "failed to execute ... No such file"를 미설치로 처리

### 검증
- `cargo build --offline` 통과
- `tool harness --target gguf --input /tmp/phase3-connect/min.gguf` 실행
- `tool harness --target onnx --input /tmp/phase3-connect/min.onnx` 실행
- `tool harness --target safetensors --input /tmp/phase3-connect/min.safetensors` 실행

## v1.0 TODO 마감: 코드 주석 3항목

### 태스크
- TODO에 남아 있던 코드 주석 3개 항목(zombie fencing/corpus reload/OOM 137 triage 분기) 반영

### 완료 기준
- `src/main.rs`에 3개 주석이 명시적으로 추가되고 빌드 성공
- `docs/todo.md` 체크 상태 갱신

### 결과
- `run_fuzz_pipeline`에 `corpus reload` 주석 추가(시작 시 스냅샷 고정 + 확장 지점 명시)
- `run_fuzz_pipeline` 상태 저장 구간에 `zombie fencing` 주석 추가(in-memory 단일 소유권 + file-queue 확장 시 처리 기준 명시)
- `execute_triage_subprocess`에 `OOM 137 triage 분기` 주석 추가 및 `infra_oom:exit_137` 힌트 문자열 기록
- `docs/todo.md`의 코드 주석 3개 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과

## v1.0 안정화: 에러 코드 규약 적용

### 태스크
- CLI 오류 출력에 에러 코드 식별자(E1xxx~E5xxx) 적용

### 완료 기준
- 주요 명령 실패 시 오류 메시지에 코드 접두사 출력
- 빌드 및 실패 경로 스모크 검증 통과

### 결과
- `main` 오류 출력 경로에 코드 접두사 추가
  - `E1001`: config prepare
  - `E2001`: prepare-target
  - `E3001`: run pipeline
  - `E3002`: harness execution
  - `E4001`: triage pipeline
  - `E5001`: report pipeline
- 종료 코드는 기존 동작을 유지하고, 메시지 표준화만 우선 적용

### 검증
- `cargo build --offline` 통과
- `tool harness --target onnx --input /tmp/does-not-exist.onnx` 실행 시 `[E3002]` 출력 확인
- 빈 data-dir에서 `tool report` 실행 시 `[E5001]` 출력 확인

## v1.0 마감: 오픈 이슈 2건 종료

### 태스크
- 하네스 내부 경로(구체 API/함수) 확정
- 법적/정책 체크리스트 구체화

### 완료 기준
- `docs/specs.md`에 타깃별 실제 호출 경로 명시
- `docs/rules.md`에 v1.0 정책 체크리스트 추가
- `docs/todo.md` 오픈 이슈 2건 체크 완료

### 결과
- `docs/specs.md` 13.2/13.3/13.4에 v1.0 구현 경로 확정 문구 추가
  - GGUF: `llama-cli -m <input> -n 1 -p hi`
  - ONNX: `onnxruntime.InferenceSession(...)`
  - safetensors: `safe_open(..., framework=\"pt\", device=\"cpu\")`
- `docs/rules.md`에 v1.0 제출 전 정책 체크리스트 6개 항목 추가
- `docs/todo.md` 오픈 이슈 2건 `[x]` 반영

### 검증
- 문서 간 정합성 확인(`docs/specs.md`, `docs/rules.md`, `docs/todo.md`)

## v1.0 릴리즈 준비: 차별점 검증 체크리스트

### 태스크
- 차별점을 수치 근거로 제시할 수 있는 운영 지표/증거 체크리스트 작성

### 완료 기준
- 핵심 4개 지표(시간당 신규 crash, 유효 crash 비율, 고유 시그니처 수, 제출 가능 리포트 수) 포함
- 각 지표에 수집 방법/판정 기준/증거 파일 경로 명시
- README에서 체크리스트 위치 링크 가능

### 결과
- `docs/roadmap.md`에 `차별점 검증 체크리스트 (릴리즈 이후)` 섹션 추가
- 지표 표에 핵심 4개 + 보조 지표(재현 성공률, 리포트 성공률, triage p95, FP 비율) 정의
- `README.md` 목표 섹션에 체크리스트 문서 링크 추가
- `docs/todo.md` 문서화 항목에 체크리스트 완료 항목 추가

### 검증
- 문서 내 핵심 4개 지표 키워드 존재 확인
- README에서 roadmap 체크리스트 링크 확인

## v1.0+ 운영 준비: 유효 코퍼스 SOP + prepare-target 점검

### 태스크
- 유효 코퍼스 준비 절차 문서 추가
- `prepare-target` 3타깃 실행 점검

### 완료 기준
- 코퍼스 준비 단계(수집/선별/배치/퍼징 시작)가 문서화됨
- `prepare-target --target {gguf,onnx,safetensors}` 실행 결과 확인

### 결과
- 신규 문서 추가: `docs/corpus-sop.md`
  - `prepare-target`은 타깃 고정 단계, 유효 코퍼스는 별도 준비 단계로 분리 정의
  - seed 수집/선별(`tool harness`), 중복 제거, 시작 명령 흐름 정리
- 링크 갱신:
  - `README.md`에 `docs/corpus-sop.md` 링크 추가
  - `docs/roadmap.md`에 코퍼스 준비 기준 섹션 추가
  - `docs/todo.md` 문서화 항목 체크 추가
- `prepare-target` 실행 결과:
  - 3타깃 모두 DNS 해석 실패(`Could not resolve host: github.com`)로 다운로드 실패
  - 기존 source tarball은 로컬에 존재 확인(`data/targets/*/source/*.tar.gz`)

### 검증
- `tool prepare-target --target gguf|onnx|safetensors` 실행 로그 확인
- `data/targets` 경로의 source 파일 존재 확인

## v1.0+ 운영 준비: prepare-target 재실행 점검

### 태스크
- `prepare-target` 3타깃 재실행 및 상태 갱신

### 완료 기준
- gguf/onnx/safetensors 실행 결과와 오류 코드 확인
- 현재 환경에서 meta 생성 가능 여부 확인

### 결과
- `tool prepare-target --target gguf|onnx|safetensors` 재실행
- 3타깃 모두 `E2001`로 실패
  - 원인: `Could not resolve host: github.com` (DNS/네트워크 제한)
- `data/targets` 하위 `meta.json`은 아직 생성되지 않음

### 검증
- 실행 stderr에 `E2001` 및 DNS 오류 문자열 확인
- `find data/targets -name meta.json` 결과 0건 확인

## v1.0+ 운영 검증: 절대시간 루프 1시간 테스트

### 태스크
- 장시간 퍼징 스크립트를 절대시간 루프 방식으로 검증
- Discord 시작/종료 알림 정상 여부 확인

### 완료 기준
- `onnx_1h` 실행이 정확히 1시간 후 종료
- 종료 코드 `exit=0`과 `[DONE]` 알림 확인

### 결과
- `scripts/run_onnx_6h.sh`를 `DURATION_HOURS` 기반 절대시간 루프로 실행
- 1시간 테스트 실행:
  - `[START] onnx_1h ts=2026-02-19T18:57:12+09:00`
  - `[DONE] onnx_1h_finished ts=2026-02-19T19:57:13+09:00 exit=0`
- 이전 6시간 세션은 수동 중지로 `exit=143` 기록(비정상 아님)

### 검증
- Discord 메시지: `[DONE] onnx 1h finished ... exit=0` 수신
- 로그 확인: `grep -E '^\\[START\\]|^\\[DONE\\]' data/longrun/run-onnx-loop-6h.log`

## v1.0+ 1순위 구현: local 개발 모드(--local)

### 태스크
- `tool run`에 `--local` 개발 모드 추가
- `--corpus-dir` 미지정 시 타깃별 seed 경로(`seeds/<target>`)를 기본값으로 사용

### 완료 기준
- `tool run --target <t> --local` 실행 시 `corpus_dir`가 `seeds/<t>`로 자동 선택
- `--corpus-dir`를 명시하면 기존 우선순위(명시값 우선) 유지

### 결과
- `src/main.rs` 변경:
  - `RunArgs`에 `--local`(bool) 추가
  - run 경로의 corpus 기본값 분기 추가:
    - `--corpus-dir` 있으면 해당 경로 사용
    - 없고 `--local`이면 `seeds/<target>` 사용
    - 둘 다 없으면 기존처럼 `./seeds` 사용
  - run 시작 로그에 `local_mode` 출력 추가
- `docs/todo.md`의 `local 개발 모드(--local) 추가` 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- run --target onnx --local --workers 1 --timeout-sec 5 --restart-limit 0 --max-jobs 1` 실행 성공
- 출력에서 `local_mode: true`, `corpus_dir: ./seeds/onnx` 확인

## v1.0+ 확장 준비: Legacy Upgrade TODO 분리

### 태스크
- 기존 v1.0/post-v1.0 TODO와 충돌하지 않게, 레거시 이식 전용 TODO 트랙을 별도 문서로 분리

### 완료 기준
- 레거시 이식 범위(탐색 엔진/UI/seed/coverage/통합검증)가 별도 체크리스트로 정의됨
- 코어 고정/확장 분리/결과 수렴/단계 검증 원칙이 명시됨

### 결과
- 신규 문서 추가: `docs/dev-todo-legacy-upgrade.md`
  - 작업 원칙(코어 고정, 확장 분리, 결과 수렴, 단계 검증) 명시
  - A~E 트랙(탐색 엔진, UI, seed, coverage, 통합검증) 체크리스트 정의
- `docs/todo.md` 상단에 레거시 업그레이드 TODO 문서 참조 링크 추가

### 검증
- 문서 존재 확인: `docs/dev-todo-legacy-upgrade.md`
- 참조 링크 확인: `docs/todo.md` 상단 문구

## v1.0+ 확장 준비: 레거시 강점 이식 TODO 보강

### 태스크
- Legacy Upgrade TODO에 레거시 강점(보고서/하네스/입력 변형) 이식 항목을 명시적으로 추가

### 완료 기준
- 보고서/하네스/변형/안전 가드 항목이 별도 체크리스트로 정의됨
- 기존 v1.0 코어 고정 원칙과 충돌하지 않음

### 결과
- `docs/dev-todo-legacy-upgrade.md`에 `F) 레거시 강점 선택 이식 (보고서/하네스/변형)` 섹션 추가
  - 보고서 템플릿 보강
  - 하네스 deep-path 패턴 이식 설계
  - 구조 인지 mutation 전략 문서화
  - 하드코딩/fallback/전역상태 이식 금지 규칙

### 검증
- `docs/dev-todo-legacy-upgrade.md`에서 `F)` 섹션 및 4개 항목 확인

## v1.0+ 확장 준비: Legacy 문서 초안 세트 작성

### 태스크
- 레거시 강점 이식 개발 전에 v1.0 스타일 문서 초안(요구사항/정책/검증/한계/테스트 로그) 선작성

### 완료 기준
- `docs/legacy/` 하위에 5개 초안 문서 생성
- 레거시 분석 기준을 README가 아닌 코드 중심으로 명시

### 결과
- 신규 문서 생성:
  - `docs/legacy/requirements.md`
  - `docs/legacy/policy.md`
  - `docs/legacy/verification.md`
  - `docs/legacy/limitations.md`
  - `docs/legacy/test-log.md`
- 분석 기준 명시:
  - 레거시의 `src/core/*`, `src/web/*`, `src/custom_mutator/*`, `targets/*/fuzz_harness*.cpp`, `tools/*` 코드를 기준으로 판단
  - README/USER_GUIDE는 보조 참고로만 사용

### 검증
- `ls docs/legacy`로 5개 파일 생성 확인
- `docs/legacy/test-log.md`에 Log #001 기록 확인

## v1.0+ 확장 준비: Legacy TODO 알림 루프 항목 추가

### 태스크
- Legacy Upgrade TODO에 크래시 알림 루프 항목을 명시적으로 추가

### 완료 기준
- `docs/dev-todo-legacy-upgrade.md`의 UI/운영 섹션에 `monitor/notifier` 기반 알림 항목이 존재

### 결과
- `docs/dev-todo-legacy-upgrade.md` B 섹션에 항목 추가:
  - `monitor/notifier 기반 크래시 알림 루프 추가(Discord/Webhook 연동)`

### 검증
- 문서 확인: `docs/dev-todo-legacy-upgrade.md` B 섹션 체크리스트

## v1.0+ 구현: run backend 옵션 뼈대 추가

### 태스크
- `tool run`에 backend 선택 옵션 추가 (`local-harness`, `aflpp`, `libfuzzer`)

### 완료 기준
- `--backend` 인자 파싱 가능
- 기본 backend는 `local-harness`로 동작 유지
- 미구현 backend 선택 시 명확한 에러 반환

### 결과
- `src/main.rs` 변경:
  - `RunArgs`에 `backend` 추가
  - `RunBackend` enum 추가 (`local-harness`, `aflpp`, `libfuzzer`)
  - `run_fuzz_pipeline`에 backend 게이트 추가(현재 `local-harness`만 허용)
  - run 시작 로그에 `backend` 출력 추가
- `docs/dev-todo-legacy-upgrade.md` A 섹션 첫 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- run --target onnx --backend local-harness --local --max-jobs 1 --workers 1 --timeout-sec 5 --restart-limit 0` 성공
- `cargo run --offline -- run --target onnx --backend aflpp --local --max-jobs 1` 실행 시 "not implemented yet" 오류 확인

## v1.0+ 구현: Docker 다중 인스턴스 래퍼(A-2) 추가

### 태스크
- `aflpp/libfuzzer` backend 선택 시 Docker 다중 인스턴스 래퍼 경로 진입

### 완료 기준
- backend별 래퍼 경로 진입/로그 출력
- docker 미설치/invalid 인자에 대한 명확한 오류 처리
- run 상태 파일 저장(`status.json`)

### 결과
- `src/main.rs` 변경:
  - `run_fuzz_pipeline`에서 `local-harness` 외 backend는 `run_container_backend_stub`로 분기
  - `run_container_backend_stub` 추가:
    - docker 명령 존재 확인
    - workers/corpus_dir 기본 검증
    - 다중 인스턴스 컨테이너 명령 스켈레톤(`backend_worker_cmd`) 출력
    - `data/runs/run-*/status.json` 생성(`state=backend_stub_not_implemented`)
    - 엔진 미연동 상태를 명확한 에러로 반환
  - run 디렉터리 ID를 밀리초 단위로 상향(`run-<unix_ms>`)하여 동시 실행 시 run 경로 충돌 방지
- `docs/dev-todo-legacy-upgrade.md` A-2 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- run --target onnx --backend local-harness --local --max-jobs 1 --workers 1 --timeout-sec 5 --restart-limit 0` 성공(회귀 없음)
- `cargo run --offline -- run --target onnx --backend aflpp --local --max-jobs 1 --workers 2` 실행 시
  - `backend_worker_cmd` 2줄 출력
  - `status.json` 생성
  - "wrapper initialized ... engine integration is not implemented yet" 오류 확인

## v1.0+ 구현: 장시간 루프 제어/완료 마커/종료 코드 기록(A-3)

### 태스크
- backend 공통 장시간 실행 루프에서 절대시간 제어 + 완료 마커 + 종료 코드 기록 지원

### 완료 기준
- 실행 시작/종료 로그 기록
- 완료 시 `.done` 마커 생성
- 종료 코드/실행 횟수/실패 횟수 기록 파일 생성

### 결과
- 신규 스크립트 추가: `scripts/run_backend_loop.sh`
  - 절대시간 루프(`DURATION_HOURS` 또는 `DURATION_SECONDS`) 지원
  - backend/target/corpus/workers/timeout/restart/max-jobs 환경변수로 제어
  - 로그 파일(`run-<tag>.log`), 완료 마커(`.done`), 종료 기록(`.exit`) 생성
  - 종료 시 벨(`\\a`) 출력
- `docs/dev-todo-legacy-upgrade.md` A-3 항목 `[x]` 처리

### 검증
- 실행:
  - `DURATION_SECONDS=8 WORKDIR=$PWD TARGET=onnx BACKEND=local-harness CORPUS_DIR=seeds/onnx WORKERS=1 TIMEOUT_SEC=5 RESTART_LIMIT=0 MAX_JOBS=1 LOG_DIR=/tmp/legacy-loop-test scripts/run_backend_loop.sh`
- 확인:
  - `/tmp/legacy-loop-test/run-onnx_local-harness_8s.log`에 `[START]/[DONE]` 존재
  - `/tmp/legacy-loop-test/run-onnx_local-harness_8s.done` 생성
  - `/tmp/legacy-loop-test/run-onnx_local-harness_8s.exit`에 `exit_code=0`, `runs=4`, `failures=0` 기록

## v1.0+ 구현: run status 스키마 호환 유지(A-4)

### 태스크
- backend 종류와 무관하게 `run status.json` 스키마를 동일하게 유지

### 완료 기준
- `local-harness`와 `aflpp/libfuzzer stub`가 동일 키셋으로 `status.json` 저장
- 기존 소비 경로에서 파싱 호환 유지

### 결과
- `src/main.rs` 변경:
  - status 작성 로직을 `write_run_status` 공용 함수로 통합
  - `RunStatusCounts` 구조체 추가
  - local/backend-stub 모두 동일 스키마로 기록:
    - `run_id`, `target`, `total`, `success`, `failed`, `timeout`, `retries`, `workers`, `timeout_sec`, `restart_limit`
- `docs/dev-todo-legacy-upgrade.md` A-4 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- run --target onnx --backend local-harness --local --max-jobs 1 --workers 1 --timeout-sec 5 --restart-limit 0` 실행 후 `status.json` 키셋 확인
- `cargo run --offline -- run --target onnx --backend aflpp --local --max-jobs 1 --workers 2` 실행 후 `status.json` 키셋 동일성 확인

## v1.0+ 구현: Seed 수집/중복제거 도구(C-1/C-3)

### 태스크
- seed 수집/배치 보조 명령 추가
- 해시 기반 중복 제거 로직 추가

### 완료 기준
- `tool seed sync`로 포맷별 seed 수집/복사 가능
- 동일 SHA-256 seed는 중복으로 자동 스킵
- `tool seed stats`로 개수/고유/중복 통계 출력

### 결과
- `src/main.rs` 변경:
  - 신규 명령 추가: `tool seed`
    - `tool seed sync --target <t> --from <dir> [--to <dir>]`
    - `tool seed stats --target <t> [--dir <dir>]`
  - `seed sync`:
    - 포맷 확장자 필터 적용
    - 대상 디렉터리 기존 seed 해시 수집
    - 중복 해시 스킵 + 신규 seed만 복사
    - 파일명 충돌 시 `-<n>` suffix로 안전 저장
  - `seed stats`:
    - 포맷별 총 개수/고유 개수/중복 개수/해시 오류 수 출력
- `docs/dev-todo-legacy-upgrade.md` C-1/C-3 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- 임시 디렉터리에서 중복 seed 포함 상태로 `tool seed sync` 실행 시 `dup_skipped` 증가 확인
- `tool seed stats` 출력에서 `total/unique/duplicates` 집계 확인

## v1.0+ 구현: Seed 하네스 선별 연동(C-2)

### 태스크
- `seed sync` 단계에 하네스 선별 루프를 연결해 유효 seed만 유지

### 완료 기준
- `tool seed sync --harness-filter` 시 입력별 하네스 검증 수행
- 하네스 실패 seed는 복사하지 않고 `invalid_skipped`로 집계

### 결과
- `src/main.rs` 변경:
  - `seed sync` 인자 `--harness-filter` 추가
  - `seed_harness_validate` 함수 추가 (`tool harness` 서브프로세스 호출)
  - 하네스 실패 seed를 `invalid_skipped`로 집계하고 복사 스킵
- `docs/dev-todo-legacy-upgrade.md` C-2 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- 정상/비정상 ONNX 혼합 입력으로 `tool seed sync --harness-filter` 실행
  - 비정상 입력이 `invalid_skipped`로 집계되는지 확인

## v1.0+ 구현: Seed 품질 리포트(개수/유효율) 출력(C-4)

### 태스크
- `seed stats`에 포맷별 품질 지표(유효/무효/유효율) 추가

### 완료 기준
- `tool seed stats` 실행 시 `valid`, `invalid`, `validated`, `valid_ratio` 출력

### 결과
- `src/main.rs` 변경:
  - `seed stats`에서 각 seed를 하네스로 검증
  - 품질 지표 집계/출력 추가:
    - `valid`
    - `invalid`
    - `validated`
    - `valid_ratio` (0~1, 소수 4자리)
- `docs/dev-todo-legacy-upgrade.md` C-4 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `tool seed stats --target onnx --dir /tmp/seed-filter-test/dst` 실행 시 품질 지표 출력 확인

## v1.0+ 구현: 대시보드 스냅샷 API(B-2)

### 태스크
- 대시보드 최소 구현 전 단계로, `data/*` 기반 read-only 스냅샷 API(JSON 출력) 추가

### 완료 기준
- `tool dashboard` 실행 시 run/triage/report 개수 및 최신 ID, metrics 요약을 JSON으로 출력

### 결과
- `src/main.rs` 변경:
  - 신규 명령 추가: `tool dashboard`
  - `run_dashboard_snapshot` 구현:
    - `data/runs`, `data/triage`, `data/reports`의 개수/최신 ID 집계
    - `data/metrics/latest.json`에서 핵심 수치 추출
    - 단일 JSON 스냅샷 출력
  - 보조 함수 추가:
    - `count_prefixed_dirs`
    - `latest_prefixed_dir_name`
    - `extract_json_number_literal`
- `docs/dev-todo-legacy-upgrade.md` B-2 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard` 실행 시 JSON 출력 및 필드 존재 확인

## v1.0+ 구현: read-only 최소 대시보드 화면(B-1)

### 태스크
- 기존 `dashboard` JSON 스냅샷을 기반으로 정적 HTML 최소 화면 제공

### 완료 기준
- `tool dashboard --format html --out <path>` 실행 시 HTML 파일 생성
- 화면에 run/triage/report 요약 및 metrics 핵심 값 표시

### 결과
- `src/main.rs` 변경:
  - `DashboardArgs` 확장: `--format json|html`, `--out`
  - 스냅샷 수집 로직 분리: `collect_dashboard_snapshot`
  - 출력 렌더러 분리:
    - `render_dashboard_json`
    - `render_dashboard_html`
  - HTML 이스케이프 함수(`html_escape`) 추가
  - html 모드에서 파일 출력 완료 메시지 제공
- `docs/dev-todo-legacy-upgrade.md` B-1 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard-snapshot.html` 실행 성공
- `/tmp/dashboard-snapshot.html` 파일 생성 및 핵심 카드/메트릭 값 렌더 확인

## v1.0+ 구현: 크래시 상세/리포트 링크 뷰(B-3)

### 태스크
- 대시보드에 최신 유효 triage(= `verdict: reproduced`) 상세와 연결된 report 경로 표시

### 완료 기준
- `tool dashboard` JSON 출력에 `crash` 섹션이 포함되어야 함
- `tool dashboard --format html` 출력에 triage/input/signature/summary/report 정보가 보여야 함

### 결과
- `src/main.rs` 변경:
  - `DashboardSnapshot`에 크래시 상세 필드 추가:
    - `latest_valid_triage`
    - `latest_valid_input`
    - `latest_valid_signature`
    - `latest_valid_summary`
    - `latest_valid_report`
  - `find_latest_reproduced_triage` 추가:
    - `data/triage/triage-*/summary.json`에서 최신 `verdict: reproduced` 항목 탐색
    - `signature_top3`의 첫 값(top1) 추출
  - `find_report_by_source_triage_id` 추가:
    - `data/reports/report-*/meta.json`의 `source_triage_id`로 report 연결
  - JSON/HTML 렌더러에 crash 상세 섹션 반영
  - 경량 JSON 문자열 파서 유틸 추가:
    - `extract_json_string_literal`
    - `extract_first_signature_top3`
    - `parse_json_string_literal_at`
- `docs/dev-todo-legacy-upgrade.md` B-3 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard` 실행 시 `crash.latest_valid_triage/input/signature_top1/summary/report` 필드 출력 확인
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard-snapshot.html` 실행 후 crash detail 카드 렌더 확인

## v1.0+ 구현: dashboard 렌더 분리(B-4 준비)

### 태스크
- `src/main.rs` 내 dashboard 렌더(HTML/JSON)를 UI 모듈 + 템플릿 파일로 분리

### 완료 기준
- dashboard 렌더 코드가 `src/ui/dashboard.rs`로 이동
- HTML 마크업이 별도 템플릿 파일(`templates/dashboard.html`)로 분리
- 기존 명령 동작/출력 필드가 유지

### 결과
- `src/main.rs` 변경:
  - `mod ui;` 추가
  - 대시보드 렌더 호출을 `ui::dashboard::render_dashboard_json/html`로 교체
  - 인라인 렌더 함수 제거
  - `DashboardSnapshot`을 모듈 접근 가능 형태(`pub(crate)`)로 조정
- 신규 파일 추가:
  - `src/ui/mod.rs`
  - `src/ui/dashboard.rs`
  - `templates/dashboard.html`
- `docs/dev-todo-legacy-upgrade.md`의 dashboard 렌더 분리 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard` 실행 후 JSON 필드(`snapshot/metrics/crash`) 유지 확인
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 실행 후 HTML 생성/필드 렌더 확인

## v1.0+ 구현: 코어 실행 로직과 UI 서버 분리(B-4)

### 태스크
- 코어 CLI(run/triage/report)와 분리된 UI 서버 엔트리포인트를 추가

### 완료 기준
- `tool ui-serve` 명령으로 별도 서버 루프를 실행
- UI 서버가 코어 로직을 직접 변경하지 않고 대시보드 스냅샷만 read-only 제공
- 엔드포인트 제공: `/healthz`, `/dashboard.json`, `/dashboard.html`

### 결과
- `src/main.rs` 변경:
  - 신규 에러 코드 `E6001` 추가
  - 신규 서브커맨드 `UiServe(UiServeArgs)` 추가 (`--bind`, 기본 `127.0.0.1:8787`)
  - `Commands::UiServe` 분기에서 `ui::server::run_ui_server` 호출
  - `AppPaths`, `collect_dashboard_snapshot`를 `pub(crate)`로 조정해 UI 모듈에서 read-only 재사용
- 신규 파일 추가:
  - `src/ui/server.rs`
    - 단순 HTTP 서버 루프
    - `GET /healthz` -> `ok`
    - `GET /dashboard.json` -> 대시보드 JSON
    - `GET /dashboard.html`/`/` -> 대시보드 HTML
  - `src/ui/mod.rs`에 `server` 모듈 export 추가
- `docs/dev-todo-legacy-upgrade.md` B-4 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- ui-serve --bind 127.0.0.1:18787` 실행 시 명령 경로/에러코드(`exit=8`) 확인
- 본 샌드박스에서는 포트 바인딩이 차단되어 HTTP 핸들러 실호출 검증은 제한됨
  - 실환경 검증 명령:
    - `tool ui-serve --bind 127.0.0.1:8787`
    - `curl http://127.0.0.1:8787/healthz`
    - `curl http://127.0.0.1:8787/dashboard.json`
    - `curl http://127.0.0.1:8787/dashboard.html`

## v1.0+ 중간정산: B-5 monitor/notifier 상태

### 태스크
- B-5(Discord/Webhook 연동) 구현 상태를 부분구현/잔여구현으로 명확히 기록

### 완료 기준
- 현재 동작하는 알림 경로와 미구현 범위를 문서에서 즉시 식별 가능
- 후속 구현자가 바로 착수 가능한 수준의 작업 단위 명시

### 결과
- 부분 구현(완료):
  - `scripts/run_onnx_6h.sh`
  - `HOOK_FILE="${HOME}/.config/bugbounty/discord_webhook"` 기반
  - `[START]`, `[DONE]` 이벤트 Discord 전송
- 미완료(잔여):
  - `scripts/run_backend_loop.sh`에 공통 notifier 미통합
  - run 결과(status/triage 기반) 신규 crash 이벤트 알림 미구현
  - B-5를 완료 처리할 통합 운영 검증 절차(1h/6h + 로그/알림 대조) 미완료

### 검증
- 기존 운영 로그 기준으로 `run_onnx_6h.sh` START/DONE 알림 수신 이력 존재
- `docs/dev-todo-legacy-upgrade.md` B 섹션에 부분구현/잔여구현 항목 동기화 완료

## v1.0+ 중간정산: 다음 구현 우선순위 재정의 (2026-03-06)

### 태스크
- 엔진 실활용 기준으로 우선순위를 재정의하고, UI 고도화 시점을 명확화

### 완료 기준
- 문서에 순서가 명시되어 이후 구현 경로가 혼선 없이 이어질 것

### 결과
- 우선순위 확정:
  1. A 실통합(`aflpp/libfuzzer`) 완료
  2. D coverage 표시 흐름 이식
  3. B-5 notifier는 보류(현재 Discord webhook 경로로 임시 운영)
  4. 레거시 수준 UI 고도화는 엔진/coverage 완료 후 진행
- `docs/dev-todo-legacy-upgrade.md`에 다음 항목 추가:
  - A 실통합 세부 항목(ONNX 1타깃 기준)
  - 레거시 수준 UI 고도화 항목(탐색/재현 동선 강화)

### 검증
- `ui-serve` 실환경 확인:
  - `cargo run --offline -- ui-serve --bind 127.0.0.1:8787`
  - `curl http://127.0.0.1:8787/healthz` -> `ok`
  - `curl http://127.0.0.1:8787/dashboard.json` 응답 확인

## v1.0+ 실행계획 확정: A 실통합 후 사용자 실환경 검증 (2026-03-06)

### 태스크
- A 실통합 구현 이후 검증 책임 경계를 문서화하고, D 진입 조건을 명확히 고정

### 완료 기준
- A 완료 판정이 "코드 구현 + 사용자 실환경 검증 결과 반영" 기준으로 정의됨
- D 및 UI 고도화 진입 순서가 고정됨

### 결과
- `docs/dev-todo-legacy-upgrade.md`에 실행 방식 섹션 추가:
  - A 구현/로컬 빌드 검증
  - 사용자 실환경 검증
  - 검증 결과 반영 후 A 완료 확정
  - D 진행, 이후 UI 고도화
- A 섹션에 `사용자 실환경 검증 결과 반영` 체크 항목 추가

### 검증
- `docs/dev-todo-legacy-upgrade.md`에 실행 방식/체크 항목 반영 확인

## v1.0+ 구현: A 실통합 코드 경로 전환(stub -> engine command)

### 태스크
- `aflpp/libfuzzer` backend를 placeholder(stub) 에러에서 실제 엔진 명령 실행 경로로 전환

### 완료 기준
- `run --backend aflpp|libfuzzer`가 환경변수 템플릿 기반 명령을 실제 실행
- 결과를 `data/runs/run-*/status.json`과 `logs/backend-engine.log`로 수렴

### 결과
- `src/main.rs` 변경:
  - `run_container_backend_stub` 제거, `run_engine_backend` 추가
  - 엔진 명령 템플릿 함수 추가:
    - `build_engine_command`
    - `shell_escape`
  - 환경변수 기반 명령 선택:
    - `TOOL_AFLPP_CMD`
    - `TOOL_LIBFUZZER_CMD`
  - 실행 결과를 `backend-engine.log`에 stdout/stderr/exit_code 기록
  - status/metrics는 기존 스키마로 유지
- `docs/dev-todo-legacy-upgrade.md`에 A 항목의 부분구현/잔여 구현 상태 반영

### 검증
- `cargo build --offline` 통과
- 템플릿 스모크:
  - `TOOL_AFLPP_CMD='echo ...; true'` 로 `run --backend aflpp` 실행 시 `success: 1` 확인
  - `TOOL_LIBFUZZER_CMD='echo ...; false'` 로 `run --backend libfuzzer` 실행 시 `failed: 1` 확인
- `run-*/logs/backend-engine.log` 생성 및 실행 명령/출력 기록 확인

### 사용자 실환경 검증 명령(다음 단계)
- AFL++ 예시:
  - `TOOL_AFLPP_CMD='docker run --rm -v \"$PWD\":/work -w /work aflpp-image bash -lc \"<afl-cmd using {corpus_dir} {run_dir} {target}>\"' cargo run --offline -- run --target onnx --backend aflpp --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1`
- libFuzzer 예시:
  - `TOOL_LIBFUZZER_CMD='bash -lc \"<libfuzzer-cmd using {corpus_dir} {run_dir} {target}>\"' cargo run --offline -- run --target onnx --backend libfuzzer --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1`
- 완료 판정:
  - status가 실패 메시지(stub)가 아닌 실제 엔진 exit 기반으로 집계
  - `backend-engine.log`에 엔진 출력이 기록

## v1.0+ 구현: D coverage 표시 흐름 이식

### 태스크
- coverage를 퍼징 코어와 분리된 별도 명령(job)으로 실행하고, 결과를 대시보드에 연결

### 완료 기준
- `tool coverage` 명령으로 별도 job 실행
- 산출물이 표준 경로(`data/coverage/coverage-*/summary.json`)에 저장
- `tool dashboard` JSON/HTML에서 최신 coverage 링크 표시

### 결과
- `src/main.rs` 변경:
  - 신규 명령 추가: `tool coverage`
  - `CoverageArgs` 추가 (`--target`, `--corpus-dir`, `--timeout-sec`, `--max-jobs`)
  - `run_coverage_job` 구현:
    - 하네스 재사용 실행
    - `logs/job-*.log` 기록
    - `summary.json` 생성(`total/success/failed/timeout/success_ratio`)
  - `collect_dashboard_snapshot` 확장:
    - `coverage_count`
    - `latest_coverage`
    - `latest_coverage_summary`
- `src/ui/dashboard.rs`, `templates/dashboard.html` 변경:
  - dashboard JSON에 `coverage` 섹션 추가
  - HTML에 Coverage Jobs/Latest coverage/Coverage summary 표시 추가
- `docs/dev-todo-legacy-upgrade.md` D 항목 4개 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- coverage --target onnx --corpus-dir seeds/onnx --timeout-sec 5 --max-jobs 3` 실행 성공
  - `./data/coverage/coverage-*/summary.json` 생성 확인
- `cargo run --offline -- dashboard` 출력에서 `coverage_count/latest_coverage/coverage.summary` 필드 확인
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 출력에서 coverage 항목 렌더 확인

## v1.0+ 구현: Adapter 기초 도입 (Engine/Target)

### 태스크
- 엔진/타깃 확장 시 전체 코어 수정을 피하기 위해 최소 어댑터 계층 도입

### 완료 기준
- backend 명령 선택이 하드코딩 분기 대신 `EngineAdapter` 경유
- target 식별자 매핑이 `TargetAdapter` 경유

### 결과
- `src/main.rs` 변경:
  - `EngineAdapter`, `TargetAdapter` 구조체 추가
  - `resolve_engine_adapter`, `resolve_target_adapter` 함수 추가
  - `build_engine_command`가 어댑터 기반으로 템플릿 치환 수행
    - env 키: `TOOL_AFLPP_CMD`, `TOOL_LIBFUZZER_CMD`
- `docs/dev-todo-legacy-upgrade.md`에 Adapter 규격 고정 항목 추가

### 검증
- `cargo build --offline` 통과
- `TOOL_AFLPP_CMD`/`TOOL_LIBFUZZER_CMD` 템플릿 스모크에서 기존 실행 결과 유지 확인

## v1.0+ 구현: A 엔진 실통합 보강 (worker 단위 실행/집계)

### 태스크
- 엔진 backend 실행을 worker 단위로 확장하여 실운영 집계 정확도를 보강

### 완료 기준
- `workers=N`일 때 엔진 명령이 worker별로 실행
- worker별 로그 파일 생성
- `status.json` 집계가 worker 단위 결과를 반영

### 결과
- `src/main.rs` 변경:
  - `run_engine_backend`에서 worker 루프 실행
  - 로그 경로: `run-*/logs/backend-engine-w<id>.log`
  - 상태 집계: `total=workers`, `success/failed/timeout` worker 합산
  - 템플릿 치환 변수 확장:
    - `{worker_id}`
    - `{worker_log}`
    - `{workdir}`
- 기존 어댑터 경로(`EngineAdapter/TargetAdapter`)는 유지

### 검증
- `cargo build --offline` 통과
- 스모크 실행:
  - `TOOL_AFLPP_CMD='...; true'` + `--workers 2` -> `success: 2`, `failed: 0`
  - `TOOL_LIBFUZZER_CMD='...; false'` + `--workers 2` -> `success: 0`, `failed: 2`
- `run-*/logs/backend-engine-w1.log`, `backend-engine-w2.log` 생성 확인
- `run-*/status.json`에 `total: 2` 반영 확인

### 실환경 검증 제약 및 다음 액션
- 현재 환경 제약:
  - `docker` 미존재
  - `afl-fuzz` 미존재
  - `clang++` 미존재
- 사용자 실환경에서 아래 검증 수행 필요:
  1. `docker --version` 또는 `afl-fuzz -V` 확인
  2. AFL++ 검증
     - `TOOL_AFLPP_CMD='<실제 afl++ 명령 템플릿>' cargo run --offline -- run --target onnx --backend aflpp --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1`
  3. libFuzzer 검증
     - `TOOL_LIBFUZZER_CMD='<실제 libfuzzer 명령 템플릿>' cargo run --offline -- run --target onnx --backend libfuzzer --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1`
  4. 판정:
     - `status.json`이 실제 엔진 exit 기준으로 집계
     - worker 로그에 엔진 출력 기록

## v1.0+ 문서화: 퍼징 호스트 의존성/실환경 검증 가이드 정리

### 태스크
- 새 퍼징 호스트에서 재현 가능한 설치/검증 절차를 문서화

### 완료 기준
- `clang` 외 필요 의존성 목록이 README에 명시
- `scripts/setup_fuzz_host.sh` 사용법과 엔진 검증 명령이 README에 포함
- to-do에 문서화 완료 항목 반영

### 결과
- `README.md` 갱신:
  - Fuzz Host 준비 섹션 추가
  - 의존성 목록(`clang`, `docker.io`, `build-essential`, `pkg-config`, `curl`, `git`, `jq`, `tmux`, `python3`, `python3-pip`) 명시
  - `scripts/setup_fuzz_host.sh` 사용법 추가
  - AFL++/libFuzzer 실환경 검증 명령 추가
- `docs/dev-todo-legacy-upgrade.md` 갱신:
  - E 섹션에 `퍼징 호스트 의존성 설치 스크립트/사용법 문서화` 완료 항목 추가

### 검증
- 사용자 실환경 확인:
  - `docker --version` 확인
  - AFL++ Docker 이미지 실행 확인
  - `clang++ --version` 확인
- 사용자 실행 결과:
  - `run --backend aflpp --workers 2` 성공 (`success: 2`, `failed: 0`)
  - `run --backend libfuzzer --workers 2` 경로 스모크 성공 (`success: 2`, `failed: 0`)
  - `status.json`/`backend-engine-w*.log` 생성 확인

## v1.0+ 보강: fresh host 완전 재현용 rustup/cargo 추가

### 태스크
- 호스트 부트스트랩 스크립트에 `rustup/cargo` 설치를 포함해 fresh host 재현성 보강

### 완료 기준
- `scripts/setup_fuzz_host.sh`만으로 시스템 의존성과 Rust 빌드 도구를 함께 준비 가능
- 레거시 업그레이드 TODO(E)에서 해당 문서화 항목이 명확히 보일 것

### 결과
- `scripts/setup_fuzz_host.sh` 갱신:
  - `cargo` 미존재 시 `rustup` 설치 수행
  - 설치 후 `source "$HOME/.cargo/env"` 및 `cargo --version` 검증 안내 추가
- `docs/dev-todo-legacy-upgrade.md` 갱신:
  - E 섹션에 호스트 의존성 문서화 항목 복구 및 세부 의존성(`rustup/cargo` 포함) 명시

### 검증
- `bash -n scripts/setup_fuzz_host.sh` 문법 통과
- 문서 키워드 확인:
  - `docs/dev-todo-legacy-upgrade.md` E 섹션에서 문서화 항목 노출

## v1.0+ 구현: 엔진 하네스 연결(AFL++/libFuzzer)

### 태스크
- 엔진 실행 템플릿을 실제 `tool harness` 호출 경로로 연결

### 완료 기준
- AFL++ 템플릿에서 `tool harness --input @@` 호출 명령 제공
- libFuzzer 템플릿에서 `tool harness`를 호출하는 드라이버 빌드/실행 성공

### 결과
- 신규 파일 추가:
  - `harnesses/libfuzzer/tool_harness_driver.cc`
  - `scripts/build_libfuzzer_tool_driver.sh`
- `build_libfuzzer_tool_driver.sh`:
  - `clang++ -fsanitize=fuzzer`로 드라이버 빌드
- README 업데이트:
  - AFL++ 하네스 연결 템플릿 명령
  - libFuzzer 하네스 연결 템플릿 명령
  - Docker 권한 이슈(`newgrp docker`) 안내

### 검증
- `scripts/build_libfuzzer_tool_driver.sh` 실행 성공
- libFuzzer 하네스 연결 실행 성공:
  - `run --backend libfuzzer` 결과 `success: 1`, `failed: 0`
  - `run-*/logs/backend-engine-w1.log` 생성
  - `run-*/status.json` 정상 집계
- AFL++ 하네스 연결:
  - 현재 실행 환경에서는 `docker.sock` 권한 제한으로 실패 재현
  - 사용자 실환경에서는 동일 템플릿 명령으로 검증 가능(권한 적용 후)

## v1.0+ 검증: A 엔진 실환경 스모크 결과 반영 (사용자 실행)

### 태스크
- 사용자 실환경에서 AFL++/libFuzzer backend 스모크 실행 결과를 A 검증에 반영

### 완료 기준
- 양 backend 모두 `status.json` 성공 집계 확인
- worker 로그 생성 확인

### 결과
- AFL++ (사용자 실행):
  - `run --backend aflpp --workers 1` 실행 성공
  - 결과: `success: 1`, `failed: 0`, `timeout: 0`
  - 산출물: `data/runs/run-1772890636825/status.json`, `logs/backend-engine-w1.log`
- libFuzzer (사용자 실행):
  - `run --backend libfuzzer --workers 2` 실행 성공
  - 결과: `success: 2`, `failed: 0`, `timeout: 0`
  - 산출물: `data/runs/run-1772884717666/status.json`, `logs/backend-engine-w1.log`, `logs/backend-engine-w2.log`

### 검증
- 사용자 터미널 출력 기준으로 수치/파일 경로 확인 완료
- `docs/dev-todo-legacy-upgrade.md` A 항목에 "실환경 스모크 완료 / 1h 검증 잔여" 상태 반영

## v1.0+ 계획: UI 고도화 작업 세분화 (안전 진행)

### 태스크
- 레거시 스타일 UI 고도화 범위를 작은 단위(UI-1~UI-5)로 분할해 작업 안정성 확보

### 완료 기준
- 각 단계마다 완료 판정이 가능한 기준이 문서에 존재
- 구현 순서가 고정되어 누락 없이 진행 가능

### 결과
- `docs/dev-todo-legacy-upgrade.md` B 섹션에 UI 세분화 단계 추가:
  - UI-1 레이아웃/카드
  - UI-2 링크 동선
  - UI-3 crash 상세 패널
  - UI-4 반응형/가독성
  - UI-5 웹 회귀 검증/문서 반영

### 검증
- 세분화 단계/완료 기준 문구 반영 확인

## v1.0+ 구현: UI-1 대시보드 레이아웃/카드 개선

### 태스크
- 레거시 스타일을 참고해 read-only 대시보드의 정보 밀도/가독성 개선

### 완료 기준
- 핵심 카드(runs/triage/reports/coverage/metrics)가 한 눈에 보이는 구조
- runtime/crash 정보가 구획화된 패널로 표시
- 모바일/데스크탑에서 레이아웃 깨짐 없이 렌더

### 결과
- `templates/dashboard.html` 변경:
  - hero 헤더 + 2컬럼 패널 구조(좌: Runtime Snapshot, 우: Crash Detail) 적용
  - 카드 스타일/색상/타이포 개선
  - 반응형 미디어 쿼리 추가(`max-width:960px`, `max-width:640px`)
  - 기존 데이터 치환 키는 유지 (`{{...}}`)
- `docs/dev-todo-legacy-upgrade.md`:
  - UI-1 항목 `[x]` 처리

### 검증
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 실행 성공
- 생성 HTML에서 핵심 섹션 렌더 확인:
  - `Read-only Dashboard`
  - `Coverage Jobs`
  - `Runtime Snapshot`
  - `Crash Detail`

## v1.0+ 구현: UI-2 링크 동선 추가(run/triage/report/coverage)

### 태스크
- 대시보드에서 run/triage/report/coverage 산출물로 바로 이동 가능한 링크 추가

### 완료 기준
- `Latest run/triage/report/coverage`가 클릭 가능 링크로 렌더
- `Summary path/Report path/Coverage summary`가 클릭 가능 링크로 렌더
- UI 서버에 파일 조회 엔드포인트 제공

### 결과
- `src/ui/dashboard.rs` 변경:
  - 링크 렌더 헬퍼 추가:
    - `id_to_file_link`
    - `file_link`
    - `url_encode`
  - runtime/crash 경로 항목을 `/file?path=...` 링크로 렌더
- `templates/dashboard.html` 변경:
  - 링크 스타일(`.path-link`) 추가
  - runtime/crash 주요 경로 필드를 `*_html` 플레이스홀더로 교체
- `src/ui/server.rs` 변경:
  - 신규 엔드포인트: `/file?path=...`
  - URL 디코딩/쿼리 파라미터 파싱 추가
  - `data_dir` 밖 접근 차단(`resolve_safe_data_path`)으로 안전 가드 적용

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 실행 성공
- 생성 HTML에서 링크 렌더 확인:
  - `/file?path=./data/runs/.../status.json`
  - `/file?path=./data/triage/.../summary.json`
  - `/file?path=./data/reports/.../report.md`
  - `/file?path=./data/coverage/.../summary.json`

## v1.0+ 구현: UI-3 상세 뷰 라우트 추가(run/triage/report/coverage)

### 태스크
- dashboard 링크 클릭 시 raw 파일만 보여주는 방식에서 상세 라우트 기반 웹 뷰로 개선

### 완료 기준
- `/run/<id>`, `/triage/<id>`, `/report/<id>`, `/coverage/<id>` 라우트 제공
- dashboard의 Latest 링크가 상세 라우트로 연결
- 상세 페이지에서 main 파일 내용과 관련 파일 링크 표시

### 결과
- `src/ui/dashboard.rs` 변경:
  - Latest 링크를 `/run|/triage|/report|/coverage` 경로로 변경
- `src/ui/server.rs` 변경:
  - 상세 엔티티 라우트 핸들러 추가: `handle_entity_view`
  - `run/triage/report/coverage`별 main 파일 매핑 및 HTML 렌더
  - 관련 파일(`/file?path=...`) 링크 표시
  - 기존 `data_dir` 경로 제한 가드 재사용
- `docs/dev-todo-legacy-upgrade.md`:
  - UI-3 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 실행 성공
- 생성 HTML에서 Latest 링크 확인:
  - `/run/run-*`
  - `/triage/triage-*`
  - `/report/report-*`
  - `/coverage/coverage-*`

## v1.0+ 구현: UI-4 반응형/가독성 조정

### 태스크
- 모바일/데스크탑 모두에서 대시보드와 상세 뷰 가독성 개선

### 완료 기준
- 대시보드 템플릿에 모바일 대응 스타일 보강
- 상세 뷰에서 긴 본문/로그를 안정적으로 읽을 수 있는 레이아웃 적용

### 결과
- `templates/dashboard.html` 변경:
  - 모바일 폰트/패딩/카드 크기 보강
  - 하단 사용 안내 문구(`footer-note`) 추가
- `src/ui/server.rs` 변경:
  - 상세 페이지 스타일 개선(고정 상단, 스크롤 가능한 본문, 모바일 폰트 조정)
  - 긴 JSON/로그 가독성 향상(`pre` 높이 제한 + overflow)
- `docs/dev-todo-legacy-upgrade.md`:
  - UI-4 항목 `[x]` 처리

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 실행 성공
- CSS/템플릿 확인:
  - `@media (max-width:640px)` 존재
  - 상세 페이지 `position:sticky`, `max-height:68vh` 적용 확인

## v1.0+ 구현: UI-5 웹 회귀 검증 스크립트 추가

### 태스크
- UI 라우트 회귀를 반복 가능하게 검증하는 스크립트 추가 및 문서 반영

### 완료 기준
- `ui-serve` 자동 기동 후 `healthz/dashboard` 및 주요 상세 링크(run/triage/report/coverage) 검증
- 검증 로그를 파일로 남기고 실패 시 원인 로그를 확인 가능

### 결과
- 신규 스크립트 정리: `scripts/check_ui_routes.sh`
  - 검증 대상:
    - `/healthz`
    - `/dashboard.html`
    - `/dashboard.json`
    - dashboard에서 추출한 `run/triage/report/coverage` 링크
  - 산출물:
    - `data/ui-check/ui-serve.log`
    - `data/ui-check/ui-routes-check.log`
  - 보강 사항:
    - 서버 준비 대기(retry) 추가
    - 서버 조기 종료/바인드 실패 시 server log tail 출력
- TODO 반영:
  - `docs/dev-todo-legacy-upgrade.md` UI-5 항목에 진행 상태/실환경 최종확인 필요 사항 기록

### 검증
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo build --offline` 통과
- 본 샌드박스에서는 포트 bind 제한으로 자동 실행 실패 확인:
  - `failed to bind '127.0.0.1:8787': Operation not permitted (os error 1)`
- 사용자 실환경에서 최종 검증 명령:
  - `scripts/check_ui_routes.sh`
 - 사용자 실환경 최종 결과(통과):
   - `[OK] http://127.0.0.1:8787/healthz`
   - `[OK] http://127.0.0.1:8787/dashboard.html`
   - `[OK] http://127.0.0.1:8787/dashboard.json`
   - `[OK] http://127.0.0.1:8787/run/run-1772890636825`
   - `[OK] http://127.0.0.1:8787/triage/triage-1771501969`
   - `[OK] http://127.0.0.1:8787/report/report-1771501975`
   - `[OK] http://127.0.0.1:8787/coverage/coverage-1772807608605`

## v1.0+ 계획 확정: UI 고도화 트랙을 G(옵션 C)로 분리

### 태스크
- Seed 트랙(C)와 충돌하지 않도록 UI 고도화 트랙 식별자를 재정의하고, 단계형 이식 계획을 문서화

### 완료 기준
- `docs/dev-todo-legacy-upgrade.md`에 `G` 트랙이 추가되고 단계별 완료 기준/검증 명령이 존재
- 로그인/회원가입 제외 범위가 명시되어 구현 범위 혼선을 방지
- CSS 전략(레거시 톤 + 현재 구조 적합성)이 문서로 고정

### 결과
- `docs/dev-todo-legacy-upgrade.md` 갱신:
  - 우선순위에 `G(옵션 C: 단계형)` 반영
  - `G) 운영형 UI 단계 이식` 신설:
    - `G-0` 디자인/스타일 가드
    - `G-1` 운영형 UI Lite
    - `G-2` 운영 UX 강화
    - `G-3` 선택 확장(Replay, Target upload/build)
  - 범위 제외 명시: 로그인/회원가입 제외
  - CSS 방향 고정:
    - 인라인 CSS 분리
    - 디자인 토큰(`:root`) 기반
    - Bootstrap 기반 운영 콘솔 톤 유지
  - 공통 가드 명시:
    - 코어 불변
    - UI/코어 분리 유지
    - 파일 접근 보안 가드 유지

### 검증
- 문서 확인:
  - `docs/dev-todo-legacy-upgrade.md`에서 `## G)` 섹션 및 `G-0~G-3` 항목 확인
  - 로그인/회원가입 제외 문구 확인
  - 단계별 완료 기준/검증 항목 존재 확인

## v1.0+ 구현: G-0-1/G-1-9 스타일 자산 분리 + 회귀 확장 완료

### 태스크
- `dashboard.html` 인라인 CSS를 정적 자산으로 분리
- UI 서버에 정적 CSS 라우트 추가
- UI 회귀 스크립트에 정적 자산 점검 추가

### 완료 기준
- `/assets/dashboard.css`가 `ui-serve`에서 200 응답
- `scripts/check_ui_routes.sh`가 자산 라우트를 포함해 `[OK]` 출력

### 결과
- `templates/assets/dashboard.css` 신설 및 스타일 이관
- `templates/dashboard.html` 인라인 `<style>` 제거, `<link rel="stylesheet" href="/assets/dashboard.css">` 적용
- `src/ui/server.rs`에 `/assets/dashboard.css` 라우트 추가
- `scripts/check_ui_routes.sh`에 `/assets/dashboard.css` 점검 항목 추가
- `docs/dev-todo-legacy-upgrade.md`에서 `G-0 스타일 자산 분리`, `G-1-9 UI 회귀 스크립트 확장`을 `[x]`로 갱신

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- 사용자 실환경 실행 결과:
  - `[OK] http://127.0.0.1:8787/healthz`
  - `[OK] http://127.0.0.1:8787/dashboard.html`
  - `[OK] http://127.0.0.1:8787/dashboard.json`
  - `[OK] http://127.0.0.1:8787/assets/dashboard.css`
  - `[OK] http://127.0.0.1:8787/run/run-1772890636825`
  - `[OK] http://127.0.0.1:8787/triage/triage-1771501969`
  - `[OK] http://127.0.0.1:8787/report/report-1771501975`
  - `[OK] http://127.0.0.1:8787/coverage/coverage-1772807608605`

## v1.0+ 구현: G-1-1 탭 셸(1차 코드 반영)

### 태스크
- 단일 페이지 대시보드에 `Dashboard/Config/Seeds/Crashes/Coverage/Triage` 탭 셸 추가

### 완료 기준
- 단일 페이지에서 6개 탭 전환 가능
- `scripts/check_ui_routes.sh` + 브라우저 수동 확인

### 결과
- `templates/dashboard.html` 변경:
  - 탭 버튼 6개(`Dashboard/Config/Seeds/Crashes/Coverage/Triage`) 추가
  - 탭별 패널(`data-panel`) 구조 추가
  - 클릭 시 활성 탭/패널을 전환하는 스크립트 추가
- `templates/assets/dashboard.css` 변경:
  - 탭 셸 스타일(`.tabs`, `.tab-btn`, `.tab-panel`) 추가
  - 모바일 구간 탭 버튼 가독성 보강

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 통과
- 생성 HTML에서 탭 셸 구조 확인:
  - 탭 버튼 6개(`Dashboard/Config/Seeds/Crashes/Coverage/Triage`)
  - 탭 패널 6개(`data-panel=dashboard|config|seeds|crashes|coverage|triage`)
  - 전환 스크립트(`querySelectorAll(".tab-btn")`) 존재
- 사용자 실환경 수동 확인 완료(탭 전환 클릭 동작 정상)

## v1.0+ 구현: G-1-2 제어 패널 + 실제 제어 API 연동(1차)

### 태스크
- 대시보드에 start/stop/status 제어 패널 추가
- UI 서버에 제어 API(`/control/status`, `/control/start`, `/control/stop`) 추가

### 완료 기준
- 시작/중지 요청 UI 제공, 상태 배지 반영
- 실제 제어 API 연동(`start/stop/status`) 포함

### 결과
- `src/ui/server.rs` 변경:
  - 제어 API 추가:
    - `GET /control/status`
    - `POST /control/start`
    - `POST /control/stop`
  - `scripts/run_backend_loop.sh` 백그라운드 실행/중지 제어 추가
  - `data/ui-control/control.state` 상태 파일(`pid/started_at/target/backend/log_file`) 관리 추가
- `templates/dashboard.html` 변경:
  - `Run Control` 패널 추가(상태 배지, Start/Stop 버튼, 로그/메시지)
  - `fetch("/control/status")`, `fetch("/control/start",{method:"POST"})`, `fetch("/control/stop",{method:"POST"})` 연동
- `templates/assets/dashboard.css` 변경:
  - 제어 패널/버튼/상태 배지 스타일 추가
- `scripts/check_ui_routes.sh` 변경:
  - `/control/status` 자동 점검 항목 추가
- `docs/dev-todo-legacy-upgrade.md` 갱신:
  - `G-1-2` 진행 상태 반영
  - `G-1-9` 진행 상태에 `/control/status` 점검 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out data/dashboard/latest.html` 통과
- 생성 HTML에서 제어 패널/연동 확인:
  - `Run Control` 패널 존재
  - `id=control-start`, `id=control-stop`, `id=control-badge` 존재
  - `/control/status`, `/control/start`, `/control/stop` 호출 스크립트 존재
- 사용자 실환경 수동 검증 완료:
  - 브라우저에서 Start/Stop 클릭 후 상태 배지 변화 확인

## v1.0+ 구현: G-1-3 기본 지표 카드/표 구성 완료

### 태스크
- runs/triage/reports/coverage/metrics 수치 카드와 최신 항목 표를 대시보드에 통합
- `dashboard.json`과 화면 값의 일치 검증

### 완료 기준
- 수치 카드 + 최신 항목 표 출력
- `dashboard.json` 값과 화면 수치/최신 항목 일치

### 결과
- `templates/dashboard.html` 변경:
  - `Dashboard` 탭에 `Operational Table` 패널 추가
  - 항목: Runs/Triage/Reports/Coverage/Metrics
  - 각 항목에 count + latest 링크/지표 텍스트 배치
- `templates/assets/dashboard.css` 변경:
  - `.table-wrap`, `.ops-table` 스타일 추가(모바일 가독성 포함)
- `docs/dev-todo-legacy-upgrade.md`에서 `G-1-3` `[x]` 반영

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format json > /tmp/dashboard.json` 실행
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 실행
- 비교 검증 결과:
  - `runs_count`, `triage_count`, `report_count`, `coverage_count` 값이 HTML 카드/표에 일치
  - `latest_run`, `latest_triage`, `latest_report`, `latest_coverage` 값이 HTML 링크에 일치
  - 결과: `[OK] dashboard.json <-> dashboard.html snapshot values match`

## v1.0+ 구현: G-1-4 Crashes 탭 목록/상세 링크 강화

### 태스크
- Crashes 탭에 최신 crash/triage 목록 추가
- triage 상세/summary 파일 링크를 즉시 열 수 있도록 링크 강화

### 완료 기준
- 최신 triage 목록 + 상세 링크 제공
- `/triage/<id>`, `/file?path=...` 링크 동작

### 결과
- `src/main.rs` 변경:
  - `DashboardSnapshot`에 `recent_triage_ids` 추가
  - `recent_prefixed_dir_names(..., \"triage-\", 8)`로 최신 triage 8건 수집
- `src/ui/dashboard.rs` 변경:
  - `recent_triage_rows` 렌더 추가
  - Crashes 탭의 `Latest valid triage`를 `/triage/<id>` 링크로 렌더
  - 각 recent triage 항목에 `summary.json`(`/file?path=...`) 링크 추가
- `templates/dashboard.html` 변경:
  - Crashes 탭에 `Recent triage list` 행 추가
- `templates/assets/dashboard.css` 변경:
  - `.mini-list`, `.sep` 스타일 추가
- `docs/dev-todo-legacy-upgrade.md`에서 `G-1-4` `[x]` 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML 확인:
  - Crashes 탭에 `Recent triage list` 출력 확인
  - `/triage/triage-*` 링크 다건 확인
  - `/file?path=./data/triage/triage-*/summary.json` 링크 다건 확인
- 사용자 실환경 수동 검증 완료:
  - `scripts/check_ui_routes.sh` 전체 `[OK]` 확인(`.../control/status` 포함)
  - Crashes 탭에서 `triage-...` 링크 -> `/triage/<id>` 열림 확인
  - Crashes 탭에서 `summary.json` 링크 -> `/file?path=...` 열림 확인

## v1.0+ 구현: G-1-5 Reports 탭 목록/열람 동선 추가

### 태스크
- Reports 탭에서 report 목록과 열람 링크 제공
- `/report/<id>` 및 `report.md` 접근 동선 강화

### 완료 기준
- report 목록/열기 링크 제공
- `/report/<id>` 및 `report.md` 열람 가능

### 결과
- `src/main.rs` 변경:
  - `DashboardSnapshot`에 `recent_report_ids` 추가
  - `recent_prefixed_dir_names(..., "report-", 8)`로 최신 report 8건 수집
- `src/ui/dashboard.rs` 변경:
  - `recent_report_rows` 렌더 추가
  - report 항목별 `/report/<id>` + `/file?path=./data/reports/<id>/report.md` 링크 생성
- `templates/dashboard.html` 변경:
  - 탭 버튼 `Reports` 추가
  - `data-panel="reports"` 패널 추가
  - `Latest report`, `Latest valid report path`, `Recent report list` 행 구성
- `docs/dev-todo-legacy-upgrade.md`에서 `G-1-5` `[x]` 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML 확인:
  - `data-tab="reports"` 버튼 존재
  - `data-panel="reports"` 패널 존재
  - `/report/report-*` 링크 다건 확인
  - `/file?path=./data/reports/report-*/report.md` 링크 다건 확인
- 사용자 실환경 수동 검증 완료:
  - `scripts/check_ui_routes.sh` 전체 `[OK]` 확인
  - Reports 탭에서 `report-...` 링크 진입 확인
  - Reports 탭에서 `report.md` 링크 열람 확인

## v1.0+ 구현: G-1-6 Coverage 탭 링크/요약 강화

### 태스크
- Coverage 탭에서 최신 coverage 링크/summary 표시 강화
- coverage 목록에서 상세/summary 접근 동선 제공

### 완료 기준
- 최신 coverage summary/링크 제공
- `/coverage/<id>` 동작 및 summary 표시 확인

### 결과
- `src/main.rs` 변경:
  - `DashboardSnapshot`에 `recent_coverage_ids` 추가
  - `recent_prefixed_dir_names(..., "coverage-", 8)`로 최신 coverage 목록 수집
- `src/ui/dashboard.rs` 변경:
  - `recent_coverage_rows` 렌더 추가
  - coverage 항목별 `/coverage/<id>` + `/file?path=./data/coverage/<id>/summary.json` 링크 생성
- `templates/dashboard.html` 변경:
  - Coverage 탭에 `Recent coverage list` 행 추가
- `docs/dev-todo-legacy-upgrade.md`에서 `G-1-6` `[x]` 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML 확인:
  - `data-panel="coverage"` 패널 존재
  - `Recent coverage list` 표시 확인
  - `/coverage/coverage-*` 링크 확인
  - `/file?path=./data/coverage/coverage-*/summary.json` 링크 확인

## v1.0+ 구현: G-1-7 Config/Seeds 탭 read-only 구성

### 태스크
- Config 탭에 현재 설정 경로 read-only 표시
- Seeds 탭에 타깃별 seed 현황(read-only) 표시

### 완료 기준
- 현재 적용 설정/seed 현황 조회 가능
- 데이터 원본(`data/*`, `seeds/*`)과 값 대조 가능

### 결과
- `src/main.rs` 변경:
  - `DashboardSnapshot`에 config/seeds 필드 추가:
    - `data_dir`, `seeds_dir`
    - `seeds_onnx_count`, `seeds_gguf_count`, `seeds_safetensors_count`, `seeds_total_count`
  - `count_seed_files` 추가로 `seeds/<target>` 확장자 기준 파일 수 집계
- `src/ui/dashboard.rs` 변경:
  - `dashboard.json`에 `config`, `seeds` 섹션 추가
  - HTML 템플릿 치환(`config_*`, `seeds_*`) 추가
- `templates/dashboard.html` 변경:
  - Config 탭: `data_dir`, `seeds_dir` 행 추가
  - Seeds 탭: onnx/gguf/safetensors/total seed 카운트 행 추가
- `docs/dev-todo-legacy-upgrade.md`에서 `G-1-7` `[x]` 반영

### 검증
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format json > /tmp/dashboard.json` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 비교 검증 결과:
  - `config.data_dir`, `config.seeds_dir` 값이 HTML Config 탭에 일치
  - `seeds.onnx_count`, `seeds.gguf_count`, `seeds.safetensors_count`, `seeds.total_count` 값이 HTML Seeds 탭에 일치
  - 결과: `[OK] config/seeds json->html values match`

## v1.0+ 구현: G-1-8 토스트/모달 기본 UX

### 태스크
- 성공/오류 피드백 토스트 추가
- 상세 정보 모달 표시 기능 추가

### 완료 기준
- 오류/성공 피드백 토스트 동작
- 상세 모달 표시 동작

### 결과
- `templates/dashboard.html` 변경:
  - Config 탭에 `UI feedback test` 버튼 추가:
    - `Success Toast`
    - `Error Toast`
    - `Open Modal`
  - Crashes 탭 `Signature top1` 영역에 `View In Modal` 버튼 추가
  - 공통 토스트 컨테이너(`toast-root`)와 모달 마크업(`ui-modal`) 추가
  - JS에 토스트/모달 유틸 추가:
    - `showToast(kind, message)`
    - `openModal(title, body)`, `closeModal()`
  - 제어 패널 연동:
    - Start/Stop 성공 시 success toast
    - Start/Stop 실패 및 상태 갱신 실패 시 error toast
- `templates/assets/dashboard.css` 변경:
  - 토스트(`.toast-*`) 스타일 추가
  - 모달(`.ui-modal-*`) 스타일 추가
  - 모바일 대응 스타일 보강
- `docs/dev-todo-legacy-upgrade.md`에서 `G-1-8` `[x]` 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML 확인:
  - `toast-root`, `ui-modal` 마크업 존재
  - `showToast(...)`, `openModal(...)` 스크립트 존재
  - `manual error toast` 강제 시나리오 버튼 존재
- 사용자 실환경 수동 검증 완료:
  - Success/Error toast 표시 동작 확인
  - Open Modal(`Config Snapshot`) 표시 동작 확인
  - 상태 갱신 실패 상황에서 error toast 표시 동작 확인

## v1.0+ 정리: G-1-10 문서/로그 동기화 완료

### 태스크
- G-1 단계 구현/검증 로그를 `progress-log`에 단계별 반영

### 완료 기준
- `G-1-1`~`G-1-9` 구현/검증 로그 확인 가능

### 결과
- 단계별 구현/검증 로그 반영 완료:
  - G-1-1 탭 셸
  - G-1-2 제어 패널/API
  - G-1-3 지표 카드/표
  - G-1-4 Crashes 탭
  - G-1-5 Reports 탭
  - G-1-6 Coverage 탭
  - G-1-7 Config/Seeds 탭
  - G-1-8 토스트/모달
  - G-1-9 회귀 스크립트 확장

### 검증
- 본 문서(`docs/progress-log.md`) 하단 연속 섹션에서 단계별 로그 확인 가능

## v1.0+ 구현: G-2-1 차트 패널(1차)

### 태스크
- 대시보드에 시계열 차트 1개 이상 렌더
- 최근 N포인트 자동 갱신

### 완료 기준
- 차트 패널 렌더
- 최근 N포인트(30) 갱신

### 결과
- `templates/dashboard.html` 변경:
  - `Metric Trend (Recent 30)` 패널 추가
  - `<canvas id="metric-chart">`, `chart-meta` 추가
  - JS 차트 로직 추가:
    - `/dashboard.json` 5초 폴링
    - `new_paths_per_hour`, `new_crashes_per_hour`, `global_error_rate_5m` 누적(최대 30)
    - 라인 차트 렌더(`drawMetricChart`)
- `templates/assets/dashboard.css` 변경:
  - 차트 패널/캔버스/메타 스타일 추가
  - 모바일 가독성 보강
- `docs/dev-todo-legacy-upgrade.md`의 `G-2-1` 진행 상태 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML 확인:
  - `Metric Trend (Recent 30)` 패널 존재
  - `metric-chart`, `chart-meta` 요소 존재
  - `/dashboard.json` 폴링 및 `drawMetricChart` 스크립트 존재
- 사용자 실환경 수동 검증 완료:
  - 차트 포인트가 5초 간격으로 누적/갱신되는지 확인

## v1.0+ 구현: G-2-2 상태 배지/타이머/진행 표시(1차)

### 태스크
- 실행 상태에 따른 UI 상태 컴포넌트 강화
- 타이머/진행률 표시 추가

### 완료 기준
- 상태 배지/타이머/진행 표시가 실행 상태에 맞게 변경

### 결과
- `src/ui/server.rs` 변경:
  - `/control/status` 응답에 `duration_seconds` 추가
  - `control.state`에 `duration_seconds` 저장/로드 추가
- `templates/dashboard.html` 변경:
  - Run Control 패널에 `Timer`, `Progress` 행 추가
  - JS에 `fmtHms` 기반 경과시간/총시간 렌더 추가
  - `duration_seconds`와 `started_at` 기반 진행률(%) 계산/렌더 추가
- `templates/assets/dashboard.css` 변경:
  - 진행바(`.progress-track`, `.progress-fill`) 및 텍스트 스타일 추가
- `docs/dev-todo-legacy-upgrade.md`의 `G-2-2` 진행 상태 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML/API 확인:
  - `control-timer`, `control-progress` 요소 존재
  - `/control/status`에 `duration_seconds` 포함
  - 타이머/진행률 계산 스크립트(`fmtHms`, ratio 계산) 존재
- 사용자 실환경 수동 검증 완료:
  - Start 후 상태 배지 `running`, 타이머 증가, 진행률 증가 확인
  - Stop 후 상태 배지 `stopped`, 타이머/진행률 초기화 확인

## v1.0+ 구현: G-2-3 목록 가독성 강화(필터/정렬 1차)

### 태스크
- crash/report 목록에 기본 필터 또는 정렬 제공

### 완료 기준
- crash/report 목록에서 기본 정렬 또는 필터 제공
- 동일 데이터로 정렬 결과 재현 가능

### 결과
- `templates/dashboard.html` 변경:
  - Crashes 탭에 `Filter triage id` 입력 + `Newest First/Oldest First` 정렬 버튼 추가
  - Reports 탭에 `Filter report id` 입력 + `Newest First/Oldest First` 정렬 버튼 추가
  - 공통 클라이언트 유틸 `bindListControls` 추가
    - 텍스트 includes 필터
    - 오름차순/내림차순 정렬 토글
- `templates/assets/dashboard.css` 변경:
  - `.filter-bar`, `.filter-input`, `.sort-btn` 스타일 추가
- `docs/dev-todo-legacy-upgrade.md`의 `G-2-3` 진행 상태 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML 확인:
  - `crash-filter-input`, `crash-sort-btn` 존재
  - `report-filter-input`, `report-sort-btn` 존재
  - `bindListControls` 스크립트 존재
- 사용자 실환경 수동 검증 완료:
  - 동일 목록에서 필터 입력 시 항목 축소 확인
  - 정렬 토글 시 `Newest First`/`Oldest First` 순서 변화 확인

## v1.0+ 구현: G-3-1 Replay UI/API(1차)

### 태스크
- 선택 crash에 대해 재현 실행 트리거 제공
- replay 결과/요약 표시

### 완료 기준
- replay 실행 트리거 가능
- 결과 표시 가능

### 결과
- `src/ui/server.rs` 변경:
  - Replay API 추가:
    - `GET /replay/status`
    - `POST /replay/start`
    - `POST /replay/stop`
  - `tool triage`를 백그라운드 replay 작업으로 실행
  - `data/ui-replay/replay.state`, `data/ui-replay/replay.log` 상태/로그 관리 추가
  - replay 완료 후 log에서 `summary:`/`verdict:` 추출 가능하도록 status 응답 구성
- `templates/dashboard.html` 변경:
  - Crashes 탭에 `Replay` 패널 추가
  - `Replay input`, `Replay status`, `Replay actions`, `Replay verdict`, `Replay summary` 표시
  - JS에 replay 상태 조회/시작/중지 연동 추가
- `scripts/check_ui_routes.sh` 변경:
  - `/replay/status` 자동 점검 항목 추가
- `docs/dev-todo-legacy-upgrade.md`의 `G-3-1` 진행 상태 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML/API 확인:
  - `Replay` 패널 존재
  - `replay-input`, `replay-badge`, `replay-verdict`, `replay-summary` 요소 존재
  - `/replay/status`, `/replay/start`, `/replay/stop` 연동 스크립트 존재
- 사용자 실환경 수동 검증 완료:
  - Replay Start 후 `status=running` 확인
  - replay 완료 후 `verdict=reproduced` 표시 확인
  - `summary: ./data/triage/triage-1773569513/summary.json` 표시 확인

## v1.0+ 구현: G-3-2 Target Prepare UI/API(1차)

### 태스크
- 공식 릴리스 기반 target prepare/build 요청 경로 제공
- Config 탭에서 상태/결과 확인

### 완료 기준
- target prepare 실행 트리거 가능
- 상태/결과 표시 가능

### 결과
- `src/ui/server.rs` 변경:
  - Target Prepare API 추가:
    - `GET /target/status`
    - `POST /target/prepare`
    - `POST /target/stop`
  - 기존 `prepare-target` CLI를 백그라운드 작업으로 재사용
  - `data/ui-target/prepare-target.state`, `data/ui-target/prepare-target.log` 상태/로그 관리 추가
  - prepare 완료 후 log에서 `meta:`, `file:`, `sha256:` 추출 가능하도록 status 응답 구성
- `templates/dashboard.html` 변경:
  - Config 탭에 `Target Prepare` 패널 추가
  - target/version/source URL 입력, 상태 배지, sha256/meta/downloaded file/log 표시
  - JS에 target prepare 상태 조회/시작/중지 연동 추가
- `templates/assets/dashboard.css` 변경:
  - `form-control` 입력 스타일 추가
- `scripts/check_ui_routes.sh` 변경:
  - `/target/status` 자동 점검 항목 추가
- `docs/dev-todo-legacy-upgrade.md`의 `G-3-1` 완료 처리 및 `G-3-2` 진행 상태 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML/API 확인:
  - `Target Prepare` 패널 존재
  - `target-kind`, `target-version`, `target-source-url` 요소 존재
  - `/target/status`, `/target/prepare`, `/target/stop` 연동 스크립트 존재
- 사용자 실환경 수동 검증 부분 완료:
  - 기본 `onnx` prepare 실행 후 결과 표시(`sha256`, `meta`, `downloaded file`) 확인
  - `Meta` 링크 클릭 시 `meta.json` 열람 확인
  - 비공식 URL 입력 시 서버 실패는 발생하는 것으로 보이나, UI에 실패 상태/메시지가 명확히 표시되지 않음
  - 후속 조치: `/target/status`에 실패 상태/메시지 노출 추가, 배지/메시지/토스트 보강 필요

## v1.0+ 구현: G-3-2 Target Prepare UI/API(실패 상태 보강)

### 태스크
- target prepare 실패 상태/메시지를 UI에 명확히 표시

### 완료 기준
- 성공/실패/중단 상태가 배지와 메시지로 구분 표시
- 실패 시 사용자에게 즉시 피드백 제공

### 결과
- `src/ui/server.rs` 변경:
  - `TargetPrepareState`에 `last_result`, `last_message` 추가
  - `/target/status`가 실행 종료 후 log를 분석해 `success`/`error` 결과를 확정하도록 보강
  - `prepare-target error:` 및 source URL 검증 실패 문구를 상태 메시지로 추출하도록 추가
  - `POST /target/stop` 시 `stopped` 상태/메시지 기록
- `templates/dashboard.html` 변경:
  - `Target Prepare` 패널에 `Message` 행 추가
  - 상태 배지를 `running/success/error/stopped/idle`에 따라 구분 표시
  - 폴링 시 `success`/`error` 전환에 맞춰 토스트 표시 추가
  - 실패 시 메시지 영역에 상세 원인 표시
- `docs/dev-todo-legacy-upgrade.md`의 `G-3-2` 진행 상태 갱신

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML/API 확인:
  - `target-message` 요소 존재
  - `/target/status`가 `result`, `message` 필드 포함
  - `target prepare completed`, `target prepare failed` 토스트 스크립트 존재
- 실환경 수동 검증 대기:
  - 비공식 URL 실행 시 `error` 배지/메시지/토스트 표시 확인

## v1.0+ 구현: G-3-2 Target Build UI/API(1차)

### 태스크
- prepare-target으로 고정된 소스를 대상으로 build 요청 경로 제공
- Config 탭에서 build 상태/결과 확인

### 완료 기준
- build 실행 트리거 가능
- 상태/로그/산출물 표시 가능

### 결과
- `scripts/build_prepared_target.sh` 추가:
  - `data/targets/<name>/<version>/source/*.tar.gz`를 풀어서 build 수행
  - `gguf(llama.cpp)`는 `cmake -S/-B` + `cmake --build`로 `bin/llama-cli`까지 확인
  - `onnx/safetensors`는 현재 `build path not yet implemented`로 명시적 미지원 처리
- `src/ui/server.rs` 변경:
  - Target Build API 추가:
    - `GET /target/build/status`
    - `POST /target/build/start`
    - `POST /target/build/stop`
  - `data/ui-target-build/target-build.state`, `data/ui-target-build/target-build.log` 상태/로그 관리 추가
  - build 완료 후 log에서 `build_dir:`, `artifact:` 추출 가능하도록 status 응답 구성
- `templates/dashboard.html` 변경:
  - Config 탭에 `Target Build` 패널 추가
  - target/version 입력, 상태 배지, message/build_dir/artifact/log 표시
  - JS에 target build 상태 조회/시작/중지 연동 추가
- `scripts/check_ui_routes.sh` 변경:
  - `/target/build/status` 자동 점검 항목 추가
- `docs/dev-todo-legacy-upgrade.md`의 `G-3-2` 진행 상태 반영

### 검증
- `cargo build --offline` 통과
- `bash -n scripts/check_ui_routes.sh` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 생성 HTML/API 확인:
  - `Target Build` 패널 존재
  - `target-build-kind`, `target-build-version` 요소 존재
  - `/target/build/status`, `/target/build/start`, `/target/build/stop` 연동 스크립트 존재
- 실환경 수동 검증 대기:
  - `gguf` build 시작 후 status/message/build_dir/artifact 표시 확인
  - `onnx` 또는 `safetensors` build 시 `error`/미지원 메시지 확인

## v1.0+ 구현: G-3-2 Target Build UI/API(onnx/safetensors 확장)

### 태스크
- `Target Build`의 남은 `onnx`, `safetensors` build 경로 구현
- 3타깃 build 완료 기준을 문서에 명확히 고정

### 완료 기준
- `gguf`: `llama.cpp` 최소 유효 build 경로 성공 + `llama-cli` 산출 확인
- `onnx`: `onnxruntime` 최소 CPU shared library build 경로 성공 + `libonnxruntime.so` 산출 확인
- `safetensors`: `safetensors` Rust crate 최소 release build 경로 성공 + `libsafetensors*.rlib` 또는 동등 산출물 확인

### 결과
- `docs/dev-todo-legacy-upgrade.md` 변경:
  - `G-3-2`의 3타깃별 build 완료 기준(`gguf/onnx/safetensors`) 명시
- `scripts/build_prepared_target.sh` 변경:
  - `onnx`: upstream `build.sh --config Release --parallel 2 --build_shared_lib --skip_tests` 경로 추가
  - `onnx`: 대표 산출물 `libonnxruntime.so` 탐색/검증 추가
  - `safetensors`: `cargo build --release --manifest-path safetensors/Cargo.toml` 경로 추가
  - `safetensors`: 대표 산출물 `libsafetensors*.rlib`/`.rmeta` 탐색/검증 추가

### 검증
- `bash -n scripts/build_prepared_target.sh` 통과
- `cargo build --offline` 통과
- `cargo run --offline -- dashboard --format html --out /tmp/dashboard.html` 통과
- 실환경 수동 검증 대기:
  - `onnx` build 성공 시 `libonnxruntime.so` 표시 확인
  - `safetensors` build 성공 시 Rust 산출물 표시 확인

## v1.0+ 운영 검증: 새 메인 퍼징 컴 ONNX local-harness 1h baseline

### 태스크
- 새 메인 퍼징 컴에서 ONNX seed/harness 환경 확인
- `local-harness` 1시간 운영 스모크 실행

### 완료 기준
- `onnxruntime` 로더 경로가 새 머신에서 실제 동작
- `run_backend_loop.sh` 1시간 실행이 `exit=0`으로 종료
- `data/longrun`에 log/done/exit 파일 생성

### 결과
- 환경:
  - host: `06-211-01`
  - WSL2 Ubuntu + Docker Desktop + Python venv(`.venv`) 사용
- seed/corpus:
  - `seeds/onnx` 이관 완료
  - `min.onnx`는 로더 경로까지 타지만 `ModelProto does not have a graph`로 비정상 seed 확인
  - `onnx_10_model.onnx`는 `library_step: onnxruntime connected (session_ok:inputs=1,outputs=1)` 확인
- 1회 스모크:
  - `./target/debug/tool run --target onnx --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1`
  - 결과: `success: 26`, `failed: 0`, `timeout: 0`, `retries: 0`
- 1시간 baseline:
  - `TARGET=onnx BACKEND=local-harness CORPUS_DIR=seeds/onnx DURATION_HOURS=1 bash scripts/run_backend_loop.sh`
  - 결과: `[DONE] onnx_local-harness_1h ... exit=0 runs=1060 failures=0`
  - 산출물:
    - `data/longrun/run-onnx_local-harness_1h.log`
    - `data/longrun/run-onnx_local-harness_1h.done`
    - `data/longrun/run-onnx_local-harness_1h.exit`

### 검증
- `./target/debug/tool harness --target onnx --input seeds/onnx/onnx_10_model.onnx`
- `cat data/runs/run-*/status.json`
- `ls -la data/runs/run-*/logs`
- `cat data/longrun/run-onnx_local-harness_1h.exit`

## v1.0+ 운영 검증: 새 메인 퍼징 컴 ONNX libfuzzer 1h

### 태스크
- 새 메인 퍼징 컴에서 `libfuzzer` backend 실환경 스모크 및 1시간 운영 검증

### 완료 기준
- `tool_harness_driver`가 `tool harness --target onnx --input @@` 경로로 정상 실행
- `run_backend_loop.sh` 1시간 실행이 `exit=0`으로 종료
- `data/longrun`에 `run-onnx_libfuzzer_1h.log/.done/.exit` 파일 생성

### 결과
- 준비:
  - `scripts/build_libfuzzer_tool_driver.sh` 실행 후 `harnesses/libfuzzer/tool_harness_driver` 생성 확인
  - 환경변수 `TOOL_LIBFUZZER_CMD='TOOL_HARNESS_TOOL=./target/debug/tool TOOL_HARNESS_TARGET=onnx TOOL_HARNESS_EXT=onnx ./harnesses/libfuzzer/tool_harness_driver -max_total_time=5 {corpus_dir} >/dev/null 2>&1'` 사용
- 1회 스모크:
  - `./target/debug/tool run --target onnx --backend libfuzzer --corpus-dir seeds/onnx --workers 1 --timeout-sec 30 --restart-limit 1`
  - 결과: `success: 1`, `failed: 0`, `timeout: 0`, `retries: 0`
- 1시간 검증:
  - `TARGET=onnx BACKEND=libfuzzer CORPUS_DIR=seeds/onnx WORKERS=1 TIMEOUT_SEC=30 RESTART_LIMIT=1 DURATION_HOURS=1 bash scripts/run_backend_loop.sh`
  - 결과: `[DONE] onnx_libfuzzer_1h ... exit=0 runs=440 failures=0`
  - 산출물:
    - `data/longrun/run-onnx_libfuzzer_1h.log`
    - `data/longrun/run-onnx_libfuzzer_1h.done`
    - `data/longrun/run-onnx_libfuzzer_1h.exit`

### 검증
- `ls -la data/longrun`
- `tail -n 40 data/longrun/run-onnx_libfuzzer_1h.log`
- `cat data/longrun/run-onnx_libfuzzer_1h.exit`

## v1.0+ 운영 검증: 새 메인 퍼징 컴 ONNX aflpp 1h + triage/report/metrics 체인

### 태스크
- 새 메인 퍼징 컴에서 `aflpp` backend 실환경 스모크 및 1시간 운영 검증
- `triage -> report -> metrics` 전체 체인 검증

### 완료 기준
- Docker 기반 `afl-fuzz` 경로가 `tool harness --target onnx --input @@`로 정상 실행
- `run_backend_loop.sh` 1시간 실행이 `exit=0`으로 종료
- `tool triage`, `tool report`, `data/metrics/latest.json` 갱신까지 확인

### 결과
- 준비:
  - 환경변수 `TOOL_AFLPP_CMD='docker run --rm -v "$PWD":/work -w /work aflplusplus/aflplusplus bash -lc "afl-fuzz -n -V 5 -i {corpus_dir} -o {run_dir}/afl-out -- /work/target/debug/tool harness --target onnx --input @@ >/dev/null 2>&1 || true"'` 사용
- 1회 스모크:
  - `./target/debug/tool run --target onnx --backend aflpp --corpus-dir seeds/onnx --workers 1 --timeout-sec 30 --restart-limit 1`
  - 결과: `success: 1`, `failed: 0`, `timeout: 0`, `retries: 0`
- 1시간 검증:
  - `TARGET=onnx BACKEND=aflpp CORPUS_DIR=seeds/onnx WORKERS=1 TIMEOUT_SEC=30 RESTART_LIMIT=1 DURATION_HOURS=1 bash scripts/run_backend_loop.sh`
  - 결과: `[DONE] onnx_aflpp_1h ... exit=0 runs=99 failures=0`
  - 산출물:
    - `data/longrun/run-onnx_aflpp_1h.log`
    - `data/longrun/run-onnx_aflpp_1h.done`
    - `data/longrun/run-onnx_aflpp_1h.exit`
- 체인 검증:
  - `./target/debug/tool triage --target onnx --input seeds/onnx/onnx_10_model.onnx --repro-retries 3 --timeout-sec 10`
  - 결과: `success_count=3`, `failed_count=0`, `timeout_count=0`, `signature_consistent=true`, `verdict=reproduced`
  - `./target/debug/tool report`는 최초 실행에서 `./data/runs/run-1775621251187/afl-out` 권한 문제(`Permission denied`)로 실패
  - 확인 결과: `data/runs/run-1775621251187/afl-out`이 `root:root` 소유였음
  - 임시 복구: `sudo chown -R fuzz:fuzz data/runs data/triage data/reports data/metrics`
  - 복구 후 `./target/debug/tool report` 재실행 성공
  - `data/metrics/latest.json` 갱신 확인:
    - `new_crashes_per_hour: 1`
    - `valid_crash_ratio: 1.0000`
    - `global_error_rate_5m: 0.0000`

### 검증
- `ls -la data/longrun`
- `tail -n 40 data/longrun/run-onnx_aflpp_1h.log`
- `cat data/longrun/run-onnx_aflpp_1h.exit`
- `cat data/triage/triage-1775630494/summary.json`
- `cat data/reports/report-1775636948/meta.json`
- `cat data/metrics/latest.json`

### 후속 메모
- Docker 기반 `aflpp` 산출물(`afl-out`)이 `root:root`로 생성되어 일반 사용자 `tool report`가 읽지 못하는 이슈 확인
- 후속 수정에서 `TOOL_AFLPP_CMD` 예시/운영 경로에 `docker run --user "$(id -u):$(id -g)" ...` 반영 검토 필요

## v1.0+ 운영 보정: aflpp Docker 산출물 권한 가드

### 태스크
- `aflpp` Docker 실행 시 `afl-out`이 root 소유로 남지 않도록 실행 템플릿 보정

### 완료 기준
- `run --backend aflpp` 템플릿에서 현재 사용자 UID/GID를 자동 주입할 수 있음
- 공개 실행 예시와 운영 규칙 문서가 동일한 템플릿 규칙을 따름

### 결과
- `src/main.rs`
  - backend 템플릿 치환에 `{docker_user_flag}` placeholder 추가
  - 현재 사용자 기준 `--user <uid>:<gid>` 값을 `id -u`, `id -g`로 계산해 주입
- `README.md`
  - `TOOL_AFLPP_CMD` 예시를 `docker run --rm {docker_user_flag} ...` 형식으로 갱신
- `docs/experiment-ops.md`
  - Docker 기반 backend는 현재 사용자 권한으로 산출물을 남긴다는 운영 규칙 추가

### 검증
- `cargo build --offline` 통과
- 실환경 재검증 대기:
  - 메인 퍼징 컴에서 `git pull && cargo build`
  - `{docker_user_flag}` 포함 `TOOL_AFLPP_CMD`로 `run --backend aflpp` 재실행
  - 생성된 `afl-out` 디렉터리 소유권이 일반 사용자로 남는지 확인

## v1.0+ 운영 검증: aflpp 권한 가드 재확인

### 태스크
- `{docker_user_flag}` 적용 후 `aflpp` 산출물 소유권이 일반 사용자로 유지되는지 재확인

### 완료 기준
- `backend_engine_cmd`에 `--user <uid>:<gid>`가 포함될 것
- 생성된 `data/runs/run-*/afl-out` 소유자가 일반 사용자일 것

### 결과
- 메인 퍼징 컴(`06-211-01`) 재검증 결과:
  - `backend_engine_cmd[w1]`에 `--user 1000:1000` 포함 확인
  - `data/runs/run-1775640062365/afl-out` 소유권 확인 결과: `fuzz:fuzz`
  - 권한 모드: `drwx------`

### 검증
- `./target/debug/tool run --target onnx --backend aflpp --corpus-dir seeds/onnx --workers 1 --timeout-sec 30 --restart-limit 1`
- `find "$LATEST_RUN" -maxdepth 2 -name afl-out -exec ls -ld {} \\;`

## v1.0+ 구조 가드: Adapter 규격 고정

### 태스크
- backend/target/result root 계약을 명시적 adapter 규격으로 고정

### 완료 기준
- backend 명령/env 키가 `EngineAdapter` 기준으로 문서화되고 유지됨
- target별 기본 seed 경로와 입력 확장자가 코드에서 단일 규칙으로 관리됨
- `run/triage/report/coverage/metrics` 결과 루트가 `ArtifactContract` 기준으로 정의됨

### 결과
- `src/main.rs`
  - `TargetAdapter`에 `seed_subdir`, `input_ext` 추가
  - `default_seed_dir()`로 기본 코퍼스 경로를 단일 규칙으로 통일
  - `ArtifactContract` 및 `artifact_contract()`/`artifact_contract_for_data_dir()` 추가
  - `run`, `coverage`, `dashboard`, `seed` 경로가 계약 헬퍼를 사용하도록 정리
- `src/report.rs`
  - 최신 triage 탐색 경로가 `ArtifactContract`를 사용하도록 정리
- `docs/experiment-ops.md`
  - `EngineAdapter`, `TargetAdapter`, `ArtifactContract` 공개 규격 추가

### 검증
- `cargo build --offline` 통과

## v1.0+ 장시간 비교: ONNX backend 6h (06-211-01)

### 태스크
- 새 메인 퍼징 컴에서 ONNX 대상 `local-harness`, `libfuzzer`, `aflpp`를 동일 조건으로 6시간 비교 실행

### 완료 기준
- 세 backend 모두 `scripts/run_backend_loop.sh` 6시간 실행이 `exit=0`으로 종료
- 각 backend별 `runs`, `failures`, `metrics/latest.json` 스냅샷을 확보

### 결과
- 공통 조건:
  - host: `06-211-01`
  - corpus: `seeds/onnx`
  - workers: `1`
  - timeout: `30`
  - restart_limit: `1`
  - duration: `6h`
- 상세 비교 표/요약 묶음:
  - [summary.md](/home/ssw/bugbounty/results/experiments/2026-04-08-onnx-backend-comparison-6h-06-211-01/summary.md)
  - [manifest.json](/home/ssw/bugbounty/results/experiments/2026-04-08-onnx-backend-comparison-6h-06-211-01/manifest.json)
  - [notes.md](/home/ssw/bugbounty/results/experiments/2026-04-08-onnx-backend-comparison-6h-06-211-01/notes.md)
- 요약:
  - `local-harness`: `exit=0`, `runs=4395`, `failures=0`
  - `libfuzzer`: `exit=0`, `runs=2628`, `failures=0`
  - `aflpp`: `exit=0`, `runs=580`, `failures=0`

### 검증
- `tail -n 40 data/longrun/run-20260408_onnx_local_6h_06-211-01.log`
- `cat data/longrun/run-20260408_onnx_local_6h_06-211-01.exit`
- `tail -n 40 data/longrun/run-20260408_onnx_libfuzzer_6h_06-211-01.log`
- `cat data/longrun/run-20260408_onnx_libfuzzer_6h_06-211-01.exit`
- `tail -n 40 data/longrun/run-20260408_onnx_aflpp_6h_06-211-01.log`
- `cat data/longrun/run-20260408_onnx_aflpp_6h_06-211-01.exit`

### 메모
- `new_paths_per_hour`는 현재 구현상 success 기반 proxy 지표이며, coverage edge 계수와 동일 의미로 해석하면 안 된다.
- 이번 6시간 결과에서는 `new_crashes_per_hour = 0`이므로, backend 간 crash 발견 성능 비교보다는 장시간 안정성/처리량 비교에 의미가 있다.
