# 실험 운영 규칙 (코딩 컴 / 메인 퍼징 컴 분리)

## 목적
- 코드 개발과 장시간 퍼징 실행을 분리해 운영을 단순하게 유지한다.
- raw 퍼징 데이터와 Git 이력을 섞지 않는다.
- 논문화/비교 실험에 필요한 정량 지표를 재현 가능하게 남긴다.

## 역할 분리

### 1) 코딩 컴
- 코드 수정
- 대시보드/UI 수정
- 공개 문서 작성
- 내부 운영 규칙/실험 규칙 설계
- 지표 정의 및 결과 요약 포맷 설계
- Git commit / push

### 2) 메인 퍼징 컴
- `git pull`
- seed 준비
- 실제 퍼징 실행
- `data/`에 raw 결과 축적
- 장시간 실험 수행
- 실험 종료 후 요약용 결과 묶음(export) 생성

## GitHub에 올릴 것
- `src/`
- `templates/`
- `scripts/`
- `README.md`
- `docs/specs.md`
- `docs/roadmap.md`
- `docs/corpus-sop.md`
- 이 문서(`docs/experiment-ops.md`)
- 선별된 공개 결과 요약(`results/experiments/<experiment_id>/`)

## GitHub에 올리지 않을 것
- `data/`
- `seeds/`
- 장시간 로그 raw 원본
- crash 원본 입력
- IDE/workspace 파일
- 개인 메모/내부 체크리스트

## 공개 규칙
- 실험 방법, 실행 명령, 지표 정의, 결과 요약 포맷은 공개 문서에 남긴다.
- 논문화에 쓰일 정량 지표 정의는 공개 문서와 일치해야 한다.
- 퍼징 실행 자체는 CLI/스크립트 기준으로 문서화한다.
- 대시보드는 운영 보조 도구로 간주하고, 실험 판정 기준은 CLI 산출물 기준으로 본다.
- Docker 기반 backend(`aflpp`)는 현재 사용자 권한으로 산출물을 남기도록 실행한다. `TOOL_AFLPP_CMD` 예시는 `docker run --rm {docker_user_flag} ...` 형식을 기준으로 유지한다.

## 내부 규칙
- 개발 우선순위와 임시 TODO는 코딩 컴에서만 관리한다.
- 실험 중 판단 메모, 중간 가설, 비교 초안은 내부 문서로 남긴다.
- 메인 퍼징 컴은 코드 수정 없이 실행 전용으로 유지한다.

## 데이터 보관 원칙

### 1) raw 데이터
- 위치: `data/`
- 예:
  - `data/runs/`
  - `data/triage/`
  - `data/reports/`
  - `data/coverage/`
  - `data/metrics/`
  - `data/longrun/`
- Git에 올리지 않는다.

### 2) seed
- 위치: `seeds/<format>/`
- Git에 올리지 않는다.
- 코퍼스는 메인 퍼징 컴 로컬 자산으로 본다.

### 3) 공유 가능한 요약 결과
- 위치: `results/experiments/<experiment_id>/`
- 필요 시 Git에 올린다.
- raw 전체를 복사하지 말고 비교/논문화에 필요한 요약만 넣는다.

## 실험 ID 규칙
- 형식: `<date>-<target>-<backend>-<duration>-<machine>`
- 예:
  - `2026-04-07-onnx-local-1h-mainbox01`
  - `2026-04-08-onnx-aflpp-1h-mainbox01`

## 권장 결과 묶음 구조
```text
results/
  experiments/
    2026-04-07-onnx-local-1h-mainbox01/
      manifest.json
      summary.md
      run-status.json
      metrics-latest.json
      triage-index.tsv
      report-index.tsv
      notes.md
```

## 결과 묶음 파일 정의

### `manifest.json`
- 실험 메타데이터
- 필수 필드:
  - `experiment_id`
  - `machine_label`
  - `target`
  - `backend`
  - `workers`
  - `timeout_sec`
  - `restart_limit`
  - `duration_hours`
  - `corpus_dir`
  - `git_commit`
  - `started_at`
  - `finished_at`

### `summary.md`
- 사람용 요약
- 최소 포함:
  - 실행 조건
  - 총 실행 결과
  - 대표 crash/triage/report
  - 실패/이상 징후
  - 다음 액션

### `run-status.json`
- 대표 run의 `status.json` 복사본

### `metrics-latest.json`
- 실험 종료 시점의 `data/metrics/latest.json` 복사본

### `triage-index.tsv`
- 최소 컬럼:
  - `triage_id`
  - `verdict`
  - `input_path`
  - `signature_top1`
  - `summary_path`

### `report-index.tsv`
- 최소 컬럼:
  - `report_id`
  - `source_triage_id`
  - `report_path`
  - `meta_path`

### `notes.md`
- 수동 검토 메모
- 환경 이슈, 재시도 이유, 특이사항 기록

## 정량 지표 규칙

### 공통 기록 항목
- `target`
- `backend`
- `workers`
- `duration`
- `machine spec`
- `corpus size`
- `seed count`
- `git_commit`

### 최소 정량 지표
- `total_runs`
- `success`
- `failed`
- `timeout`
- `retries`
- `new_crashes_per_hour`
- `valid_crash_ratio`
- `reproduced_count`
- `report_count`
- `unique_signature_count`

### 논문화용 추가 지표
- `time_to_first_crash`
- `time_to_first_reproduced`
- `triage_processing_time_p95`
- `report_success_ratio`
- `backend별 동일 시간 대비 crash/유효율 비교`

## 실험 종료 후 정리 절차
1. `data/longrun/*.log`, `*.exit`, `*.done` 확인
2. 최신 run 상태 파일 확인
3. `data/metrics/latest.json` 백업
4. triage/report 목록 인덱스 생성
5. `results/experiments/<experiment_id>/`에 요약 묶음 생성
6. raw 데이터는 메인 퍼징 컴에 유지, 필요 시 별도 압축 보관

## 대시보드 사용 기준
- 대시보드는 운영 상태 확인과 보조 제어에 사용한다.
- 실험 판정은 `status.json`, `summary.json`, `report.md`, `metrics/latest.json`을 기준으로 한다.
- JSON 링크 의존이 남아 있어도 실험 증거 기준은 유지한다.
- 향후 UI 개선은 “실험 비교/정량 지표 시각화”를 우선한다.

## 현재 운영 판단
- 현재 대시보드는 운영 콘솔로는 충분히 유용하다.
- 논문화/비교 실험 UI로는 아직 부족하다.
- 다음 보강 우선순위:
  1. 실험 단위 summary/export 뷰
  2. backend/target 비교 테이블
  3. unique signature / reproduced ratio 시각화
  4. raw JSON 링크 의존 축소
