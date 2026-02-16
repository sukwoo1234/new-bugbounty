# 진행 로그 (1.0)

이 문서는 세션 중 진행한 작업을 `태스크 / 완료 기준 / 결과 / 검증` 형식으로 기록한다.

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
