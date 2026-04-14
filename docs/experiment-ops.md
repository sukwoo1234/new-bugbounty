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

## 엔진 실통합 완료 기준

### 대상
- `local-harness`
- `libfuzzer`
- `aflpp`

### 공통 완료 조건
- ONNX seed corpus가 준비되어 있을 것 (`seeds/onnx`)
- `tool harness --target onnx --input <seed>`가 실제 라이브러리 연결 경로까지 도달할 것
- `tool run --target onnx --backend <backend> ...` 1회 스모크가 `success > 0`, `failed = 0`으로 종료할 것
- `scripts/run_backend_loop.sh` 1시간 실행이 `exit=0`으로 종료할 것
- `data/longrun/run-onnx_<backend>_1h.log/.done/.exit`가 생성될 것

### backend별 실행 명령
- `local-harness`
```bash
TARGET=onnx BACKEND=local-harness CORPUS_DIR=seeds/onnx DURATION_HOURS=1 bash scripts/run_backend_loop.sh
```
- `libfuzzer`
```bash
export TOOL_LIBFUZZER_CMD='TOOL_HARNESS_TOOL=./target/debug/tool TOOL_HARNESS_TARGET=onnx TOOL_HARNESS_EXT=onnx ./harnesses/libfuzzer/tool_harness_driver -max_total_time=5 {corpus_dir} >/dev/null 2>&1'
TARGET=onnx BACKEND=libfuzzer CORPUS_DIR=seeds/onnx WORKERS=1 TIMEOUT_SEC=30 RESTART_LIMIT=1 DURATION_HOURS=1 bash scripts/run_backend_loop.sh
```
- `aflpp`
```bash
export TOOL_AFLPP_CMD='docker run --rm {docker_user_flag} -v "$PWD":/work -w /work aflplusplus/aflplusplus bash -lc "afl-fuzz -n -V 5 -i {corpus_dir} -o {run_dir}/afl-out -- /work/target/debug/tool harness --target onnx --input @@ >/dev/null 2>&1 || true"'
TARGET=onnx BACKEND=aflpp CORPUS_DIR=seeds/onnx WORKERS=1 TIMEOUT_SEC=30 RESTART_LIMIT=1 DURATION_HOURS=1 bash scripts/run_backend_loop.sh
```

### 실패 시 복구 절차
- `docker.sock permission denied`
  - `newgrp docker` 또는 새 셸에서 재시작
- `onnxruntime unavailable`
  - `.venv` 생성 후 `onnxruntime` 설치
- `afl-out`가 `root:root`
  - 임시 복구: `sudo chown -R <user>:<group> data/runs data/triage data/reports data/metrics`
  - 지속 방지: `TOOL_AFLPP_CMD`에 `{docker_user_flag}` 포함 유지
- `report` 실패 시
  - 최신 triage 존재 여부 확인
  - 최신 run 디렉터리 및 `afl-out` 소유권 확인
  - 복구 후 `./target/debug/tool report` 재실행

### 산출물 스키마
- `run`
  - `data/runs/run-<id>/status.json`
  - `data/runs/run-<id>/logs/job-*.log`
  - `data/runs/run-<id>/logs/backend-engine-w<id>.log` 또는 동등 로그
  - `data/runs/run-<id>/afl-out/` (`aflpp`만)
- `longrun`
  - `data/longrun/run-<target>_<backend>_<duration>.log`
  - `data/longrun/run-<target>_<backend>_<duration>.done`
  - `data/longrun/run-<target>_<backend>_<duration>.exit`
- `triage`
  - `data/triage/triage-<id>/summary.json`
  - `data/triage/triage-<id>/attempt-<n>.log`
- `report`
  - `data/reports/report-<id>/report.md`
  - `data/reports/report-<id>/crash_report.txt`
  - `data/reports/report-<id>/repro.sh`
  - `data/reports/report-<id>/meta.json`
- `metrics`
  - `data/metrics/latest.json`
  - `data/metrics/events.jsonl`

## Adapter 규격

### EngineAdapter
- backend 식별자와 환경변수 키를 1:1로 고정한다.
- 현재 표준:
  - `local-harness`: 내부 실행 경로
  - `aflpp`: `TOOL_AFLPP_CMD`
  - `libfuzzer`: `TOOL_LIBFUZZER_CMD`
- 템플릿 placeholder 표준:
  - `{target}`
  - `{backend}`
  - `{corpus_dir}`
  - `{workers}`
  - `{worker_id}`
  - `{timeout_sec}`
  - `{restart_limit}`
  - `{run_dir}`
  - `{workdir}`
  - `{worker_log}`
  - `{docker_user_flag}` (`aflpp` Docker 경로 전용)

### TargetAdapter
- target 식별자와 seed/corpus 기본 경로를 1:1로 고정한다.
- 현재 표준:
  - `gguf` -> `seeds/gguf`, 입력 확장자 `gguf`
  - `onnx` -> `seeds/onnx`, 입력 확장자 `onnx`
  - `safetensors` -> `seeds/safetensors`, 입력 확장자 `safetensors`

### ArtifactContract
- 결과 루트는 아래 5개로 고정한다.
  - `data/runs`
  - `data/triage`
  - `data/reports`
  - `data/coverage`
  - `data/metrics`
- 신규 backend나 target을 추가해도 최종 산출물은 위 루트 중 하나로 수렴해야 한다.
- UI/대시보드와 report 파이프라인은 위 루트만 신뢰한다.

## 운영 스크립트 (간소화)

### 장시간 실행 래퍼
- `scripts/run_long.sh`를 사용해 긴 환경변수 명령을 단축한다.
- 예시:
```bash
bash scripts/run_long.sh --target onnx --backend aflpp --hours 6 --tag 20260408_onnx_aflpp_6h_06-211-01
```

### Discord notifier 공통 설정
- `scripts/run_backend_loop.sh` 기준으로 START/DONE/FAIL/ALERT 알림을 공통 전송한다.
- 우선순위:
  - `DISCORD_WEBHOOK` 환경변수 (직접 지정)
  - `HOOK_FILE` 경로 파일 (기본: `~/.config/bugbounty/discord_webhook`)
- ALERT 기준:
  - 각 run 반복의 최신 `status.json`에서 `failed > 0` 또는 `timeout > 0`
- 예시:
```bash
export DISCORD_WEBHOOK='https://discord.com/api/webhooks/...'
bash scripts/run_long.sh --target onnx --backend local-harness --hours 1 --tag 20260414_onnx_notify_smoke_06-211-01
```

### 결과 수집 래퍼
- `scripts/collect_longrun.sh`로 `log/exit/metrics snapshot`을 한 번에 정리한다.
- 예시:
```bash
bash scripts/collect_longrun.sh --tag 20260408_onnx_aflpp_6h_06-211-01
```

### 실험 요약 Exporter v1
- `scripts/export_experiment_summary.sh`로 결과 번들(`results/experiments/<experiment_id>/`)을 생성한다.
- 예시:
```bash
bash scripts/export_experiment_summary.sh \
  --experiment-id 2026-04-08-onnx-aflpp-6h-06-211-01 \
  --machine-label 06-211-01 \
  --target onnx \
  --backend aflpp \
  --duration-hours 6 \
  --corpus-dir seeds/onnx \
  --workers 1 \
  --timeout-sec 30 \
  --restart-limit 1 \
  --metrics-file /tmp/metrics-20260408_onnx_aflpp_6h_06-211-01.json \
  --notes "onnx 6h backend comparison export"
```

### 비교 지표 템플릿(고정)
- 실험 비교는 아래 고정 컬럼을 기준으로 `summary.md` 테이블/`manifest.json`에 저장한다.
  - `total_runs`, `success`, `failed`, `timeout`, `retries`
  - `new_crashes_per_hour`, `valid_crash_ratio`
  - `reproduced_count`, `report_count`, `unique_signature_count`
  - `new_paths_per_hour`(proxy), `global_error_rate_5m`
- 주의:
  - `new_paths_per_hour`는 현재 proxy metric이며 true coverage로 해석하지 않는다.

### seed fetch 자동화
- `scripts/seed_fetch.sh`로 seed 다운로드/검증/정리를 자동화한다.
- 기본 동작:
  - https + allowlist host 검증
  - 선택적 SHA256 검증
  - archive 추출 후 target 확장자 파일 수집
  - `tool seed sync --harness-filter` + `tool seed stats` 실행
- 예시:
```bash
bash scripts/seed_fetch.sh \
  --target gguf \
  --url https://huggingface.co/<official-org>/<repo>/resolve/<rev>/<file>.gguf \
  --sha256 <expected_sha256>
```

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
