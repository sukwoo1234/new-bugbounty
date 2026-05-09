# 개발 로드맵 (v1.0)

## 목표
- GGUF/ONNX/safetensors 퍼징 → 재현/검증 → 리포트까지 가능한 CLI 툴 완성

## Phase 0: 준비 (완료)
- 문서 기반 합의 확정 (first.md, docs/todo.md)

## Phase 1: 스캐폴딩/기초 파이프라인
- CLI 골격 (run/triage/report + list/show/export)
- 기본 경로/설정 로딩 (`./data`, `./seeds`)
- 타깃 다운로드/메타 저장(버전/해시 기록)
- 컨테이너 실행/재현 환경 고정

## Phase 2: 타깃별 하네스 통합
- GGUF 하네스 (llama.cpp 파서)
- ONNX 하네스 (onnxruntime)
- safetensors 하네스 (공식 라이브러리)

## Phase 3: 퍼징/재현/검증
- 퍼징 실행 파이프라인 (병렬 8개 기본)
- 재현 규칙 적용 (3회 재현, 스택 상위 3프레임 동일)
- 실패 모드 처리 (재시도/플레이키/타임아웃)

## Phase 4: 리포트/보관/지표
- 리포트 자동 생성 (요약/재현/환경/스택/해시)
- 보관 정책 적용 (30일, 로그 zstd, core dump OFF)
- 운영 지표(커버리지/크래시/유효율) 수집

## Phase 5: 안정화
- 샘플 리포트 1개 확정
- 문서 보강 및 TODO 업데이트

---

## Phase 종료 체크리스트 (공통)
- `cargo build` 통과
- 기본 CLI 동작: `tool --help`, `tool run/triage/report` 실행 성공
- 기본 경로 확인: `./data`, `./seeds`
- 최소 스모크 테스트 1개(더미 입력/더미 실행)
- Phase 종료 시 리팩토링 1회 수행

## Phase별 추가 체크(예시)
- Phase 1: CLI 인자/옵션 파싱 안정, 에러 메시지 기본 일관성
- Phase 2: 타깃 다운로드/해시 기록 1건 성공
- Phase 3: 재현 3회 로직/프레임 비교 동작
- Phase 4: 리포트 샘플 1개 생성

## 로그 포맷 (권장)
- JSON Lines (구조화 로그)
- 필드 예: `ts`, `level`, `event`, `msg`, `target`, `run_id`, `crash_id`
- 이벤트 예: `run.start`, `triage.start`, `repro.ok`, `report.write`

## 에러 코드 규약 (권장)
- 1xxx: CLI/입력
- 2xxx: 파일/스토리지
- 3xxx: 실행/컨테이너
- 4xxx: 재현/검증
- 5xxx: 리포트

## 1.0 릴리즈 기준 (배포 가능 상태)
- CLI 전 기능 동작
- 재현 3회/프레임 비교 검증 통과
- 리포트 생성 1개 이상
- 정책/스코프 준수 체크 완료
- 기본 에러/로그 표준 적용 완료

## 코퍼스 준비 기준 (운영)
- `prepare-target`으로 타깃 버전/해시를 먼저 고정한다.
- 실제 seed corpus 준비 절차는 `docs/corpus-sop.md`를 따른다.
- seed는 포맷별 분리(`seeds/gguf`, `seeds/onnx`, `seeds/safetensors`)를 기본으로 한다.

## 차별점 검증 체크리스트 (릴리즈 이후)
- 목적: README 차별점(Deep/Auto-Verification/Exploitability/Reproducibility)을 수치 근거로 제시
- 수집 기간: 최소 7일 이상 동일 타깃/동일 리소스 조건
- 비교 대상: 기존 베이스라인(이전 툴/이전 설정)과 동일 corpus로 A/B 비교

| 지표 | 수집 방법 | 목표/판정 기준 | 근거 파일 |
| --- | --- | --- | --- |
| 시간당 신규 crash 수 | `data/metrics/latest.json`의 `new_crashes_per_hour` 추적 | 베이스라인 대비 증가 또는 동등 + 유효율 개선 | `data/metrics/latest.json`, `data/metrics/events.jsonl` |
| 성공 실행 proxy | `successful_runs_per_hour_proxy` 추적 | true coverage가 아닌 처리량 proxy로만 사용 | `data/metrics/latest.json`, `data/metrics/events.jsonl` |
| 유효 crash 비율 | `data/triage/triage-*/summary.json` 기준 `valid_crash_ratio = reproduced / total_crashes`; 근거 crash가 없으면 `not_available`, legacy status가 없으면 `legacy_unverified` | 베이스라인 대비 상승 | `data/metrics/latest.json`, `data/triage/triage-*/summary.json` |
| 중복 제거 후 고유 시그니처 수 | `summary.json`의 `signature_top3` 해시 유니크 집계 | 동일 실행시간 대비 고유 시그니처 증가 | `data/triage/triage-*/summary.json` |
| 실제 제출 가능한 리포트 수 | 정책 체크 + 증거 번들 충족 리포트 카운트 | 주간 제출 후보 수 증가 | `data/reports/report-*/report.md`, `data/reports/report-*/meta.json` |
| 재현 성공률 | triage verdict 중 `reproduced` 비율 | 베이스라인 대비 상승 | `data/triage/triage-*/summary.json` |
| 자동 리포트 성공률 | triage 완료 대비 report 생성 성공 비율 | 95% 이상 | `data/reports/report-*`, 실행 로그 |
| 심각도 후보 추천 | sanitizer/signal/OOM/timeout 패턴 기반 `suggested_*` 필드 기록 | 자동 확정이 아닌 수동 검토 후보로만 사용 | `data/reports/report-*/meta.json`, `data/reports/report-*/report.md` |
| triage 처리시간 p95 | triage 시작~summary 저장 시간 측정 | 베이스라인 대비 악화 없음 | `data/triage/triage-*` 타임스탬프 |
| False Positive 비율 | 제출 전 수동 검토에서 반려된 비율 | 지속 하락 | 내부 리뷰 로그/제출 이력 |

### 제출용 산출물 체크
- 지표 요약 1페이지: 핵심 4개 지표(신규 crash/유효율/고유 시그니처/제출 가능 리포트 수)
- 대표 증거 3건: `summary.json`, `report.md`, `repro.sh` 각 1건
- 제출 번들: `manifest.json`과 `report-*-evidence.zip` 포함
- Docker 실행 정책: AFL++ Docker 템플릿에 `--network none`, memory/CPU/pids 제한 적용; `--read-only`는 writable volume 분리 후 적용
- 비교 그래프: 베이스라인 vs 현재(최소 7일)
