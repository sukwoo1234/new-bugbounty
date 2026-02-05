# 개발 로드맵 (v1.0)

## 목표
- GGUF/ONNX/safetensors 퍼징 → 재현/검증 → 리포트까지 가능한 CLI 툴 완성

## Phase 0: 준비 (완료)
- 문서 기반 합의 확정 (first.md, docs/todo.md, docs/rules.md)

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
