# Implementation Specs (v1)

이 문서는 구현자를 위한 참고용 명세다. 수치/규칙/필드 정의를 여기에 모아둔다.

## 0) Default Values (Tables)

### 0.1) Queue/Heartbeat
| Item | Default | Notes |
| --- | --- | --- |
| scan_delay | 0~100ms | pending 스캔 전 랜덤 딜레이 |
| heartbeat_interval | 10s | processing/<job_id>.alive 갱신 |
| stale_threshold | 60s | 미갱신 시 stale 판정 |
| retry_limit | 1 | stale 재시도 횟수 |

### 0.2) Runtime/Resource
| Item | Default | Notes |
| --- | --- | --- |
| libfuzzer_runs | 10000 | -runs=10000 기준 재실행 |
| error_exitcode | 77 | 크래시 종료 코드 |
| crash_loop_window | 5s | 비정상 종료 집계 윈도우 |
| crash_loop_threshold | 10 | 윈도우 내 비정상 종료 횟수 |
| memory_limit | 4GB | 컨테이너 레벨 제한 |

### 0.3) Repro/Triage
| Item | Default | Notes |
| --- | --- | --- |
| exec_timeout | 60s | 재현 실행 시간 제한 |
| hang_timeout | 30s | 무응답 판정 |
| repro_retries | 3 | 재현 시도 횟수 |
| flaky_threshold | 1/3 | 3회 중 1회 이하 재현 |
| oom_retry | 1 | OOM 137 재시도 횟수 |

### 0.4) Reporting/Retention
| Item | Default | Notes |
| --- | --- | --- |
| log_excerpt | 50+50 | 상위 50줄 + 하위 50줄 |
| retention_days | 30 | 보관 기간 |
| log_rotation | 10MB | stdout/stderr 로테이션 단위 |

## 1) Queue & Job Files
- 디렉터리: ./data/queue/pending, processing, done, failed, quarantine, quarantine/broken
- 상태 전이: pending -> processing -> done
- 스캔/선택/이동은 atomic rename 기반으로 처리한다.
- 스캔 전 0~100ms 랜덤 딜레이를 적용한다.
- rename 실패(이미 이동됨/파일 없음)는 정상 흐름으로 기록한다.
- 파일명: <job_id>.json
- job_id = payload canonical JSON의 SHA-256
- payload_checksum = job payload canonical JSON의 SHA-256
- canonical JSON 예시:
```json
{
	"schema_version": "1.0",
	"job_type": "run",
	"target": {
		"name": "onnxruntime",
		"version": "v1.23.2",
		"target_binary_hash": "<sha256>"
	},
	"input": {
		"path": "./seeds/min.onnx",
		"sha256": "<sha256>"
	},
	"engine": "libfuzzer",
	"seed": {
		"prng_seed": 123456789
	},
	"timeout": {
		"exec_timeout": 60,
		"hang_timeout": 30
	},
	"container_image": {
		"name": "fuzz/onnxruntime",
		"digest": "<sha256>"
	},
	"options": {
		"network_policy": "none"
	}
}
```
- created_at, history는 canonical JSON에서 제외한다.
- checksum 불일치/파싱 불가/0바이트 파일은 quarantine로 이동한다.
- quarantine에는 reason.txt에 원인을 기록한다.
- Stale Job Recovery는 heartbeat 기반으로 판단한다.
- heartbeat 파일: processing/<job_id>.alive
- heartbeat 갱신 주기: 10초
- stale 판정 기준: 60초 이상 미갱신
- stale job은 failed로 이동하고 재시도는 1회만 허용한다.
- 재시도 시 retry_count를 증가시키고 pending으로 되돌린다.
- done 디렉터리는 샤딩한다(예: done/YYYY/MM/DD/ 또는 done/af/3d/).
- pending 스캔은 파티셔닝 없이 임의 선택으로 처리한다.
- history 배열에 상태 전이와 타임스탬프를 기록한다.
- exit code 137은 OOM으로 판단하고 reason.txt에 기록한다.

## 2) Container/Executor Policies
- run_container는 job type에 따라 network_policy를 적용한다.
- network_policy 기본값: run=none, triage=none, report=bridge
- 실행 전 타깃 바이너리 해시를 검증한다(컨테이너 내부 sha256 비교).
- seed 입력은 read-only로 마운트한다.
- 크래시/로그/아티팩트는 writeable 볼륨으로 호스트에 동기화한다.
- 퍼징 임시 파일은 /dev/shm 사용을 우선한다.
- 신규 코퍼스는 host 공유 볼륨에 저장하고 재실행 시 재사용한다.

### 2.1) Platform Interfaces/Traits
- QueueTrait: enqueue, claim, ack, nack, heartbeat
- EngineTrait: run_fuzz, minimize, triage
- ExecutorTrait: run_container, collect_exit, stream_logs, record_exit_reason
- StorageTrait: put_artifact, get_artifact, list_artifacts
- 구현 선택은 config 기반 factory로 분리한다.

## 3) Fuzzing Runtime Defaults
- LibFuzzer 리셋 기준: -runs=10000 (바이너리 재실행)
- 종료 코드 정책: -error_exitcode=77
- 정상 종료: 0, 크래시: 77, 그 외: 시스템 오류로 처리
- Crash Loop Detection: 최근 5초 내 10회 이상 비정상 종료 시 failed
- 메모리 상한: 4GB (컨테이너 레벨)
- ASan 옵션: abort_on_error=1, symbolize=1, detect_leaks=0
- 헤더 보호 범위: GGUF 8바이트, ONNX 4바이트
- custom mutator 우선순위: 1.5
- fixup 허용: 헤더 매직 복구, 길이/오프셋 보정
- 스레드 억제 환경 변수: OMP_NUM_THREADS, MKL_NUM_THREADS, OPENBLAS_NUM_THREADS, NUMEXPR_NUM_THREADS, VECLIB_MAXIMUM_THREADS
- Bootstrap seed는 ./seeds에 배치한다.
- LLM 보조는 퍼징 루프 외부에서만 사용한다(Seed/Dictionary/Mutation guide).

### 3.1) Harness Constraints
- GGUF 헤더 보호 범위: 8바이트 고정
- ONNX 헤더 보호 범위: 4바이트 고정
- 스레드 억제: OMP_NUM_THREADS, MKL_NUM_THREADS, OPENBLAS_NUM_THREADS, NUMEXPR_NUM_THREADS, VECLIB_MAXIMUM_THREADS = 1
- 입력 정책: 하네스는 로컬 파일만 사용하고, 다운로드는 별도 단계로 분리한다
- 필요 시 CPU pinning을 적용한다

## 4) Repro/Triage Policies
- 재현 기준: 동일 입력/환경에서 3회 재현 성공
- 동일 입력 판정: 입력 바이트 해시 동일
- 스택 판정: 상위 3프레임 동일
- 스택 정규화 기본 스킵: asan, libc, libstdc++, libgcc, libfuzzer
- 스택 정규화는 주소/오프셋을 제거하고 모듈명+심볼 기준으로 비교한다.
- Strict 기본값: 컨테이너 이미지/라이브러리 버전 고정, 환경 변수 화이트리스트, PRNG 시드 고정 및 기록
- Debug override 허용: 재현 목적의 한시적 변경은 허용하되 로그/메타에 기록한다
- 재현 타임아웃: 60초, hang 판정: 30초 무응답
- 재시도 횟수: 3회
- flaky 기준: 3회 중 1회 이하 재현
- 재현 실패 처리: 보류 1회 재시도 후 실패 시 폐기
- ASan 빌드 크래시 발생 시 Release 빌드로 동일 입력 재실행
- Release 빌드에서 비정상 종료(SEGV/Abort 등) 발생 시 High Confidence
- 기준 미달 시 Manual Review 큐로 이동
- Exploitability Triage: crashwalk -> GEF exploitable -> 간이 판정 -> gdb-exploitable(호환 시)
- OOM 137 처리: 기본 infra_oom, 동일 입력/환경 3회 연속 재현 시 DoS 후보
- OOM 137은 1회 재시도 후 failed로 이동

## 5) Report Generation
- 템플릿 구조: Summary -> Reproduction Steps -> PoC -> Impact -> Exploit Scenario -> Value
- Evidence Bundle 파일: crash_report.txt, repro.sh, meta.json
- 로그/스택 축약: 상위 50줄 + 하위 50줄
- 키워드 포함: ERROR, WARNING, FATAL, SUMMARY, AddressSanitizer, UBSAN, SEGV, SIGABRT, panic, stack trace, backtrace, OOM, out of memory, timeout, hang, assert, abort, leak
- 매핑 우선순위: 로그/스택 > 재현 기록 > 환경 메타
- Summary 구성: 타깃/버전 + 취약점 유형 + 결과 1줄
- Reproduction Steps 순서: 이미지 태그+해시, PRNG 시드, 타임아웃, 실행 커맨드

## 6) Observability/Health
- status.json 주기 저장
- status.json 필드: 큐 상태 카운트, 워커 상태, 최근 오류 요약
- Global Error Rate는 최근 5분 기준으로 계산
- 운영 지표: 시간당 신규 경로 수, 시간당 신규 크래시 수, 유효 크래시/전체 크래시 비율
- self-test: tool self-test로 전체 파이프라인 검증(성공/타임아웃/크래시/오류 시나리오 포함)

## 7) Storage/Retention
- 보관 기간: 30일
- 코어덤프: 기본 OFF
- 로그 압축: zstd
- 실패/플레이키 크래시는 보관하지 않음
- Disk Full GC 우선순위
- repro_count 0인 임시 크래시 로그 우선 삭제
- 중복 크래시는 로그 우선 삭제, 입력 유지
- stdout/stderr 로그는 대형 파일부터 삭제
- stdout/stderr 로그는 워커 레벨에서 로테이션(10MB 단위)

## 8) CLI Defaults
- 기본 명령: tool run, tool triage, tool report
- 결과 조회: list, show <id>, export <id>
- 기본 저장 경로: ./data
- 기본 입력 디렉터리: ./seeds
- 기본 타임아웃: 60초
- 워커 역할 분리: triage 전용 1개 이상, report 전용 옵션

## 9) Build/Dev
- 하네스 빌드: Dockerfile 템플릿 + fuzz_target 교체
- pre-built base image 기본 전략
- ccache 볼륨 마운트 옵션 지원
- Dev Container local 모드 사용

## 10) Target Sources & Versions
- 공식 배포본만 사용하고 버전/해시/라이선스를 기록한다.
- 공식 채널 예시: llama.cpp Releases, onnxruntime Releases, safetensors Releases
- 버전 고정: llama.cpp b7921, onnxruntime v1.23.2, safetensors v0.7.0
- 버전 변경 시 문서/메타 갱신 및 파일 해시 재기록

## 11) Data Schemas

### 11.1) Crash Record (Required)
```json
{
	"id": "onnx/RCE/20260206-153012-01",
	"target": "onnxruntime v1.23.2",
	"schema_version": "1.0",
	"target_binary_hash": "<sha256>",
	"input": {
		"path": "./data/artifacts/crash.bin",
		"sha256": "<sha256>"
	},
	"stack_top3": [
		"libonnxruntime.so!Ort::Run",
		"libonnxruntime.so!InferenceSession::Run",
		"libonnxruntime.so!SequentialExecutor::Execute"
	],
	"signal_exit": "SEGV",
	"time": "2026-02-10T12:34:56Z"
}
```

### 11.2) Repro Record (Required)
```json
{
	"repro_count": "3/3",
	"container_image": {
		"name": "fuzz/onnxruntime",
		"digest": "<sha256>"
	},
	"repro_env": {
		"OMP_NUM_THREADS": "1",
		"MKL_NUM_THREADS": "1"
	}
}
```

### 11.3) Report Record (Required)
```json
{
	"summary": "onnxruntime v1.23.2: Heap Buffer Overflow leads to SEGV",
	"steps": [
		"docker run --rm ...",
		"./fuzz_target ./data/artifacts/crash.bin"
	],
	"impact": "Out-of-bounds write with controlled size",
	"vuln_category": "Heap Buffer Overflow",
	"component": "GraphExecutor",
	"function": "SequentialExecutor::Execute"
}
```

### 11.4) Classification/ID Rules
- 디렉터리 우선 분류: ./data/bugs/<target>/<vuln_type>/
- 파일 이름: YYYYMMDD-HHMMSS-XX
- crash_id: 경로+파일명으로 정의
- 동일 크래시 묶음 기준: stack_top3 해시

### 11.5) Optional Fields
- fuzz_run_id, dedup_hash, artifact_paths

## 12) Storage Layout
- 장기 보관 전제: 입력/로그/스택/환경 정보를 분리 저장
- 크래시 단위 디렉터리 구조(타깃/해시/타임스탬프 기준)
- 재현 가능한 최소 세트(입력+환경+재현 커맨드) 우선 보관
- 보관 기간 30일, 코어덤프 기본 OFF, 로그 zstd 압축, 실패/플레이키 미보관

## 13) Harness Plan (v1)

### 13.1) Common Flow
1. 입력 파일 로드
2. 포맷 식별/매직 확인
3. 파서/로더 진입
4. 핵심 경로 1~2개 호출
5. 결과 요약(성공/실패/크래시) 기록
- 하네스 API/함수명은 구현 단계에서 확정한다

### 13.2) GGUF Harness
- 목표: 헤더/메타데이터/텐서 인덱스 파싱 경로
- 흐름: 파일 열기 -> 헤더 파싱 -> KV 메타 파싱 -> 텐서 디렉터리 순회
- 라이브러리: llama.cpp 파서 사용

### 13.3) ONNX Harness
- 목표: protobuf 디코드 + 그래프/노드 순회
- 흐름: 파일 로드 -> protobuf 파싱 -> Graph/Node 순회 -> 기본 검증
- 라이브러리: onnxruntime 사용

### 13.4) safetensors Harness
- 목표: 헤더 JSON 파싱 + 텐서 메타 확인
- 흐름: 파일 로드 -> 헤더 JSON 파싱 -> 각 텐서 오프셋/크기 검증
- 라이브러리: 공식 safetensors 라이브러리 사용

### 13.5) Post-1.0 Strategy
- 포맷만 고정하고 구현 라이브러리는 추후 재선정

## 14) Ops Convenience Options
- 장시간 운영 시 corpus distillation 옵션을 둔다
- tool merge로 LibFuzzer -merge=1 작업을 주기 실행한다
- merge 전 RAM 여유분을 체크하고 필요 시 퍼징 워커를 일시 중단한다
