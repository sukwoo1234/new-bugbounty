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
- 현재 실행 환경에서 DNS 해석이 실패하여 실제 원격 다운로드는 미검증
  - 예: `Could not resolve host: github.com`

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
- `tool harness` 3포맷 실행 시 `direct_step` 출력 확인
  - onnxruntime: `No module named 'onnxruntime'`
  - safetensors: `No module named 'safetensors'`
  - gguf: `llama-cli not installed`

### 제한 사항
- 현재 환경에서는 네트워크/DNS 제한으로 필요한 패키지 설치 및 다운로드가 불가해
  직접 연결이 "코드 경로 준비 + 미설치 감지" 단계까지 진행됨

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

## 다음 우선순위
1. 하네스 실제 라이브러리 연결(Phase 3 잔여)
2. 퍼징 실행 파이프라인(Phase 4)
3. 재현/검증 파이프라인(Phase 5)

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
