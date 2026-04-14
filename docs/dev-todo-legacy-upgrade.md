# 개발 TODO (Legacy Upgrade Track)

이 문서는 기존 `docs/dev-todo.md`와 분리된 업그레이드 트랙이다.
목표는 v1.0 코어를 유지한 상태에서 레거시의 탐색력/시연성을 안전하게 흡수하는 것이다.

## 현재 우선순위 (2026-03-06 결정)
1. A 실통합: `aflpp/libfuzzer`를 stub 단계에서 실제 엔진 실행 단계로 완료
2. D Coverage 표시 흐름 이식
3. B-5 notifier는 보류(현재 `run_onnx_6h.sh` + Discord webhook 운영으로 임시 유지)
4. 레거시 수준 UI 고도화는 엔진/coverage 완료 후 진행
5. UI 고도화는 `G(옵션 C: 단계형)` 트랙으로 분리해 진행

## 실행 방식 (A -> D -> UI)
- A 실통합은 먼저 코드 구현/로컬 빌드 검증을 완료한다.
- 이후 엔진 바이너리/도커 환경 의존 검증은 사용자 실환경에서 수행한다.
- 사용자 검증 결과를 기준으로 A 완료 체크를 확정하고 D 단계로 진입한다.
- D 완료 후 레거시 수준 UI 고도화를 진행한다.

## 작업 원칙 (고정)
- [ ] 코어 고정: `triage/report/retention/metrics` 로직은 직접 변경하지 않는다.
- [ ] 확장 분리: 기능 추가는 `run backend`, `ui`, `seed`, `coverage` 모듈로 분리한다.
- [ ] 결과 수렴: 신규 경로 결과는 기존 `data/runs`, `data/triage`, `data/reports`, `data/metrics` 스키마로 저장한다.
- [ ] 단계 검증: 각 항목마다 실행 가능한 검증 명령 1개 이상을 기록한다.

## A) 탐색 엔진 확장 (레거시 탐색력 이식)
- [x] `run` backend 옵션 설계 (`local-harness`, `aflpp`, `libfuzzer`)
- [x] Docker 기반 다중 인스턴스 실행 래퍼 추가
- [x] 장시간 실행(1h/6h) 제어 + 완료 마커 + 종료 코드 기록
- [x] 기존 run status 포맷과 호환 유지
- [x] `aflpp` ONNX 1타깃 실통합(엔진 실행/산출물 수렴/1h 검증)
  - 코드 경로 구현 완료: `TOOL_AFLPP_CMD` 템플릿 기반 엔진 명령 실행/로그/status 수렴
  - worker 집계 구현 완료: `workers` 단위 실행, `backend-engine-w<id>.log` 기록
  - 하네스 연결 명령 정리 완료: `afl-fuzz -n ... -- /work/target/debug/tool harness --target onnx --input @@`
  - 실환경 스모크 완료: `success: 1`, `failed: 0`, `status.json/logs` 확인
  - 실환경 1h 검증 완료: 새 메인 퍼징 컴(`06-211-01`)에서 `exit=0`, `runs=99`, `failures=0`
- [x] `libfuzzer` ONNX 1타깃 실통합(엔진 실행/산출물 수렴/1h 검증)
  - 코드 경로 구현 완료: `TOOL_LIBFUZZER_CMD` 템플릿 기반 엔진 명령 실행/로그/status 수렴
  - worker 집계 구현 완료: `workers` 단위 실행, `backend-engine-w<id>.log` 기록
  - 하네스 연결 드라이버 구현 완료: `harnesses/libfuzzer/tool_harness_driver.cc`
  - 실환경 스모크 완료: `success: 2`, `failed: 0`, `status.json/logs` 확인
  - 실환경 1h 검증 완료: 새 메인 퍼징 컴(`06-211-01`)에서 `exit=0`, `runs=440`, `failures=0`
- [x] 실통합 완료 기준 문서화(실행 명령, 실패 시 복구, 산출물 스키마)
  - 공개 문서 반영 완료: `docs/experiment-ops.md`
  - 포함 내용: backend별 1h 검증 명령, 실패 시 복구 절차, `run/longrun/triage/report/metrics` 산출물 스키마
- [x] 사용자 실환경 검증 결과 반영(성공/실패 로그를 `docs/progress-log.md`에 기록 후 체크 확정)
- [x] Adapter 규격 고정(확장성 가드)
  - 코드 반영 완료: `TargetAdapter` 기본 seed 경로/입력 확장자, `ArtifactContract` 결과 루트 고정
  - 공개 문서 반영 완료: `docs/experiment-ops.md`
  - `EngineAdapter`: backend별 실행 명령/env 키(`TOOL_AFLPP_CMD`, `TOOL_LIBFUZZER_CMD`) 표준화
  - `TargetAdapter`: target 식별자/입력 계약 표준화
  - `ArtifactContract`: 결과를 `data/runs|triage|reports|coverage` 스키마로 강제 수렴

## B) UI/대시보드 이식 (리커버링 가치 상)
- [x] read-only 대시보드 최소 화면 추가 (run/triage/report/metrics 요약)
- [x] 기존 산출물 경로(`data/*`) 기반 API 추가
- [x] 크래시 상세/리포트 링크 뷰 추가
- [x] dashboard 렌더를 `src/main.rs`에서 분리 (UI 모듈 + 템플릿 파일 구조)
- [x] 코어 실행 로직과 UI 서버를 분리 유지
- [ ] monitor/notifier 기반 크래시 알림 루프 추가(Discord/Webhook 연동)
  - 부분 구현 완료: `scripts/run_onnx_6h.sh`에서 `~/.config/bugbounty/discord_webhook` 파일 기반 START/DONE 알림 전송
  - 잔여 구현: `scripts/run_backend_loop.sh` 공통 notifier 통합, run 결과 기반 crash 이벤트 알림, 운영 검증(1h/6h) 로그 기준 명문화
- [ ] 레거시 수준 UI 고도화(탐색/재현 동선 강화)
  - 대시보드 카드/상세/링크 UX 개선
  - run/triage/report 흐름의 웹 네비게이션 연결
  - coverage/seed/크래시 상태의 화면 통합 표시
  - 단계 세분화(안전 진행):
    - [x] UI-1: 대시보드 레이아웃/카드 개선(읽기 전용, 기존 데이터 유지)
      - 완료 기준: runs/triage/reports/coverage/metrics 핵심 카드 가독성 개선
    - [x] UI-2: run/triage/report/coverage 링크 동선 추가
      - 완료 기준: summary/report/status 경로를 클릭 가능한 링크로 표기
    - [x] UI-3: crash 상세 패널 강화
      - 완료 기준: latest crash input/signature/summary/report를 구획화해 표시
    - [x] UI-4: 반응형/가독성 조정
      - 완료 기준: 모바일(폭 390px)과 데스크탑에서 레이아웃 깨짐 없음
    - [x] UI-5: 웹 회귀 검증 + 문서 반영
      - 완료 기준: `ui-serve` 실환경 확인, `progress-log` 검증 로그 기록
      - 진행 상태: `scripts/check_ui_routes.sh`로 사용자 실환경 검증 완료(healthz/dashboard/run/triage/report/coverage 전부 `[OK]`)

## C) Seed 도구 이식 (아이디어 반영)
- [x] seed 수집/배치 보조 명령 추가
- [x] 하네스 선별 루프와 연동해 유효 seed만 유지
- [x] 중복 제거(해시 기반) 도구 추가
- [x] 포맷별 seed 품질 리포트(개수/유효율) 출력
- [x] seed fetch 자동화 스크립트 추가 (`scripts/seed_fetch.sh`)
  - 진행 상태: https+allowlist+sha256 검증, archive 추출, target 확장자 수집, `seed sync --harness-filter` 연동 구현 완료

## D) Coverage 표시 흐름 이식
- [x] coverage 실행을 별도 job/명령으로 분리
- [x] coverage 산출물 경로 표준화
- [x] 대시보드에서 coverage 결과 링크 표시
- [x] 퍼징 코어 경로와 분리된 실패 처리 적용

## E) 통합 검증/문서
- [x] 1시간 운영 스모크 (run -> triage -> report -> metrics)
  - 진행 상태: 새 메인 퍼징 컴(`06-211-01`)에서 ONNX 1h(`local-harness` `runs=1060`, `libfuzzer` `runs=440`, `aflpp` `runs=99`) + GGUF 1h(`local-harness` `runs=1484`, `libfuzzer` `runs=445`, `aflpp` `runs=194`) 모두 `exit=0/failures=0` 확인. ONNX 기준 `triage -> report -> metrics` 체인 검증 완료
- [ ] 6시간 운영 검증 (장시간 루프/중단/복구)
  - 진행 상태: 새 메인 퍼징 컴(`06-211-01`)에서 ONNX 6h(`local-harness` `runs=4395`, `libfuzzer` `runs=2628`, `aflpp` `runs=580`) + safetensors 6h(`local-harness` `runs=763`, `libfuzzer` `runs=743`, `aflpp` `runs=5804`) + GGUF 6h(`local-harness` `runs=8826`, `libfuzzer` `runs=2656`, `aflpp` `runs=1107`) 모두 `exit=0/failures=0` 확인. 장시간 루프 안정성은 확인됐고, 명시적 중단/복구 시나리오 검증은 잔여
- [x] CLI 운영 간소화 래퍼 구현 (`scripts/run_long.sh`, `scripts/collect_longrun.sh`)
  - 완료 기준: backend/target/hours/tag만으로 장시간 실행 가능, 종료 후 metrics snapshot 자동 보존
  - 진행 상태: 스크립트 구현 완료 + `docs/experiment-ops.md` 사용법 반영
- [x] Exporter v1 구현 (`scripts/export_experiment_summary.sh`)
  - 완료 기준: `results/experiments/<experiment_id>/`에 `manifest.json`, `summary.md`, `run-status.json`, `metrics-latest.json`, `triage-index.tsv`, `report-index.tsv`, `notes.md` 자동 생성
  - 진행 상태: 스크립트 구현 완료 + 기본 인자/출력 스키마 동작 검증(정적 확인) + `docs/experiment-ops.md` 사용법 반영
- [ ] 비교 지표 수집 템플릿 작성 (신규 crash, 유효율, 고유 시그니처)
- [ ] 완료 항목을 `docs/progress-log.md`에 단계별 기록
- [x] 퍼징 호스트 의존성 설치 스크립트/사용법 문서화 (`scripts/setup_fuzz_host.sh`, `README.md`)
  - 포함 의존성: `rustup/cargo`, `clang`, `docker.io(선택)`, 기본 빌드/운영 패키지
- [x] GGUF harness 비대화형 안정화
  - 완료 기준: `tool harness --target gguf` 실행 시 `>` 반복 출력 없이 종료
  - 진행 상태: `llama-cli` 기반 probe를 `llama-gguf-hash` 비대화형 probe로 전환, 퍼징 머신(`06-211-01`) 재검증 완료
- [ ] crash artifact 관리 정책 추가(대량 생성 대비)
  - 후보: 중복 해시 정리, 보관 상한, 자동 압축/정리, 의미 없는 빈 crash 필터

## F) 레거시 강점 선택 이식 (보고서/하네스/변형)
- [ ] 보고서 강화: v1.0 증거 스키마 유지 + 레거시식 가독성 섹션(요약/영향/재현 안내) 템플릿 보강
- [ ] 하네스 강화: 레거시 ONNX/GGUF의 deep-path 진입 패턴을 run backend 확장안으로 정리
- [ ] 변형 강화: 포맷별 구조 인지 mutation 전략(ONNX op/attr, GGUF body)을 신규 모듈 설계로 문서화
- [ ] 안전 가드: 레거시 하드코딩/fallback/전역상태 패턴은 코어 경로 이식 금지 규칙 명시

## G) 운영형 UI 단계 이식 (옵션 C)
- 목표: 레거시의 운영 UX(탭/상태/제어/차트 감성)를 현재 v1.0 코어 안정성을 유지한 채 단계적으로 이식
- 비범위: 로그인/회원가입 기능은 이 트랙에서 제외

### G-0) 디자인/스타일 가드 (선행)
- [x] 스타일 자산 분리: 인라인 CSS를 `templates/assets/`로 이동
  - 완료 기준: `templates/dashboard.html`에 `<link rel="stylesheet" ...>`만 남고 인라인 `<style>` 제거
  - 검증: `ui-serve`에서 스타일 파일 200 응답
  - 진행 상태: 코드 반영 + 실환경 검증 완료(`scripts/check_ui_routes.sh`에서 `/assets/dashboard.css` `[OK]`)
- [ ] 디자인 토큰 고정: 운영 콘솔 테마 변수 정의
  - 완료 기준: 색상/간격/반경/그림자/타이포 변수(`:root`)가 CSS 파일에 정의
  - 검증: 카드/탭/배지/테이블에 공통 변수 사용 확인
- [ ] 레거시 톤 정렬: Bootstrap 기반 컴포넌트 규칙 수립
  - 완료 기준: 탭/카드/모달/토스트 스타일이 단일 규칙으로 정리
  - 검증: 문서에 컴포넌트별 스타일 규칙 표기

### G-1) 운영형 UI Lite (우선 구현)
- [x] G-1-1 탭 셸 추가 (`Dashboard/Config/Seeds/Crashes/Coverage/Triage`)
  - 완료 기준: 단일 페이지에서 6개 탭 전환 가능
  - 검증: `scripts/check_ui_routes.sh` + 브라우저 수동 확인
  - 진행 상태: 탭 셸/패널/전환 스크립트 반영 + 브라우저 수동 전환 확인 완료
- [x] G-1-2 제어 패널 추가 (start/stop/status badge)
  - 완료 기준: 시작/중지 요청 UI 제공, 상태 표기 반영
  - 검증: 클릭 이벤트/상태 렌더 로그 확인
  - 구현 원칙: 단계형 이식으로 1차 UI(stub) 가능하되, 본 항목 완료 처리는 실제 제어 API(`start/stop/status`) 연동까지 포함
  - 진행 상태: `/control/status|start|stop` API + 대시보드 제어 패널 연동 완료, 실환경 start/stop 클릭 동작 확인 완료
- [x] G-1-3 기본 지표 카드/표 구성
  - 완료 기준: runs/triage/reports/coverage/metrics 수치 카드 + 최신 항목 표 출력
  - 검증: `dashboard.json` 값과 화면 수치 일치
  - 진행 상태: `Operational Table` 패널 추가 및 `dashboard.json`↔`dashboard.html` 수치/최신 항목 일치 검증 완료
- [x] G-1-4 Crashes 탭
  - 완료 기준: 최신 crash/triage 목록 + 상세 링크 제공
  - 검증: `/triage/<id>`, `/file?path=...` 링크 동작
  - 진행 상태: recent triage 목록(최신 8건) + `summary.json` 상세 링크 제공, 최신 valid triage 링크화 완료
- [x] G-1-5 Reports 탭
  - 완료 기준: report 목록/열기 링크 제공
  - 검증: `/report/<id>` 및 `report.md` 열람 확인
  - 진행 상태: Reports 탭 추가 + recent report 목록(최신 8건) 및 `report.md` 링크 제공 완료
- [x] G-1-6 Coverage 탭
  - 완료 기준: 최신 coverage summary/링크 제공
  - 검증: `/coverage/<id>` 동작 및 summary 표시 확인
  - 진행 상태: Coverage 탭에 recent coverage 목록(최신 8건) + `summary.json` 링크 제공 완료
- [x] G-1-7 Config/Seeds 탭(초기 read-only)
  - 완료 기준: 현재 적용 설정/seed 현황 조회 가능
  - 검증: 데이터 원본(`data/*`, `seeds/*`)과 값 대조
  - 진행 상태: `data_dir/seeds_dir` 경로 및 타깃별 seed 개수(onnx/gguf/safetensors/total) 표시 완료
- [x] G-1-8 토스트/모달 기본 UX
  - 완료 기준: 오류/성공 피드백 토스트, 상세 모달 표시
  - 검증: 강제 실패 시나리오에서 에러 토스트 출력
  - 진행 상태: Start/Stop 실패/상태갱신 실패 시 에러 토스트, 성공 시 성공 토스트, 시그니처/설정 상세 모달 표시 구현 완료 + 실환경 수동 확인 완료
- [x] G-1-9 UI 회귀 스크립트 확장
  - 완료 기준: 탭/핵심 라우트/정적 자산까지 자동 점검
  - 검증: `scripts/check_ui_routes.sh` 결과에 신규 점검 항목 포함
  - 진행 상태: `/assets/dashboard.css`, `/control/status` 자동 점검 항목 추가 완료 + 실환경 `[OK]` 확인 완료
- [x] G-1-10 문서/로그 동기화
  - 완료 기준: `progress-log`에 단계별 구현/검증 기록 반영
  - 검증: 해당 항목의 명령/결과 로그 확인 가능
  - 진행 상태: G-1-1~G-1-9 단계별 구현/검증 로그 반영 완료

### G-2) 운영 UX 강화
- [x] G-2-1 차트 패널(throughput/paths/crash 추이)
  - 완료 기준: 시계열 차트 1개 이상 렌더 + 최근 N포인트 갱신
  - 검증: 샘플 데이터/실데이터 전환 검증
  - 진행 상태: `Metric Trend (Recent 30)` 차트 패널( `/dashboard.json` 5초 폴링, paths/crashes/error_rate 시계열 렌더) 구현 + 실환경 갱신 확인 완료
- [x] G-2-2 상태 배지/타이머/진행 표시
  - 완료 기준: 실행 상태에 따라 UI 상태 컴포넌트 변경
  - 검증: run 시작/종료 시 상태 변화 확인
  - 진행 상태: `/control/status`의 `duration_seconds` 연동, 타이머(`elapsed/total`) 및 진행바(%) 렌더 구현 + 실환경 start/stop 상태 변화 확인 완료
- [x] G-2-3 테이블 가독성 강화(필터/정렬 최소 1개)
  - 완료 기준: crash/report 목록에서 기본 정렬 또는 필터 제공
  - 검증: 동일 데이터로 정렬 결과 재현
  - 진행 상태: Crashes/Reports 탭에 텍스트 필터 + 최신/오래된 순 정렬 토글 구현 + 실환경 재현 확인 완료

### G-3) 선택 확장 (필요 시)
- [x] G-3-1 Replay UI/API
  - 완료 기준: 선택 crash에 대해 재현 실행 트리거 + 결과 표시
  - 검증: 정상/실패 1회씩 재현 결과 기록
  - 진행 상태: Crashes 탭 Replay 패널 + `/replay/status|start|stop` API 구현 완료, 실환경에서 `verdict=reproduced`, `summary.json` 표시 확인 완료
- [ ] G-3-2 Target upload/build UI/API
  - 완료 기준: 공식 릴리스 기반 target prepare/build 요청 경로 제공(보안 가드 포함)
    - `gguf`: `llama.cpp` 최소 유효 build 경로 성공 + 대표 산출물(`llama-cli`) 확인
    - `onnx`: `onnxruntime` 최소 CPU shared library build 경로 성공 + 대표 산출물(`libonnxruntime.so`) 확인
    - `safetensors`: `safetensors` Rust crate 최소 release build 경로 성공 + 대표 산출물(`libsafetensors*.rlib` 또는 동등 산출물) 확인
  - 검증: 비허용 파일/경로 차단 확인
  - 진행 상태: Config 탭 `Target Prepare` 패널 + `/target/status|prepare|stop` API 구현 완료. `Target Build` 패널 + `/target/build/status|start|stop` API 구현 진행 중

### G 트랙 공통 가드
- [ ] 코어 불변: `run/triage/report/metrics` 코어 처리 함수 직접 수정 금지
- [ ] UI 서버/코어 분리 유지: UI는 read-mostly + 명시적 제어 API만 호출
- [ ] 보안 가드 유지: 파일 접근은 `data_dir` 범위 제한 유지
- [ ] 단계 완료 시 `docs/progress-log.md`에 검증 로그 필수 기록
