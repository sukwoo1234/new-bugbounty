# Bug Bounty Fuzzing Platform (v2 Renewal)

> "퍼징으로 찾았다"에서 끝내지 않고, **재현·검증·리포트까지 자동화**하는 버그바운티용 퍼징 플랫폼.

## 먼저 읽기
- 설계/결정: [first.md](first.md)

## 기존 툴 대비 차별점 (Differentiators)
- **Deep & Structured Fuzzing**: 구조 인지형 mutator/harness로 얕은 파싱 에러가 아니라 깊은 경로의 메모리 오염을 겨냥한다.
- **Auto-Verification**: 동일 컨테이너에서 3회 재현 검증하고, 증거 번들/리포트를 자동 생성해 제출 품질을 보장한다.
- **Exploitability Triage**: ASan/Release 교차 검증과 스택/레지스터 분석으로 RCE 가능성을 등급화한다.
- **Reproducibility by Design**: 환경 고정/해시 기록으로 재현성을 강화한다.
- **LLM Assist (Out of Loop)**: 퍼징 루프 외부에서 Seed/Dictionary/Mutation guide를 보조한다.

### 목표 (Goals)
- 구조 인지형 mutator/harness로 더 깊은 경로를 타겟한다.
- 기존 툴 대비 재현 성공률/제출 승인률을 수치로 개선한다.
- 차별점 근거 지표 체크리스트는 내부 문서에서 관리한다.

## RCE 탐지 방법론 (요약)
- 핵심 본체는 **하네스/뮤테이터/triage**이며, 공개 방향은 [first.md](first.md)에 정리한다.
- **Format-Aware Mutator**: 헤더/메타/오프셋/길이 필드를 의도적으로 변조해 깊은 경로를 자극한다.
- **Targeted Harness**: mmap/텐서 디코딩/메모리 할당 경로를 직접 통과하도록 하네스를 설계한다.
- **Exploitability Triage**: 레지스터/스택/PC 오염 여부를 분석해 RCE 후보 등급을 부여한다.

## 핵심 목표
- 대상 포맷: **GGUF / ONNX / safetensors**
- 유효 버그 기준: **SEGV/Abort + 동일 입력 3회 재현 + 상위 3프레임 동일**
- 자동화 범위: **퍼징 실행 → 크래시 감지 → 재현 검증 → 리포트 초안 생성**

## 시스템 아키텍처
- Fuzz Manager: 컨테이너 실행/헬스/재시작 관리
- Job Queue: 파일 기반 작업 분배/상태 전이
- Artifact Store: 크래시/재현/증거 번들 저장

## 문서 가이드
- 공개 문서: [first.md](first.md)
- 퍼징컴 이관: [docs/fuzzing-pc-migration.md](docs/fuzzing-pc-migration.md)
- 그 외 운영 계획, 상세 명세, 실험 기록, 후속 계획은 내부 문서로 관리한다.

## CLI (확정)
- `tool run`, `tool triage`, `tool report`
- 결과 조회: `list`, `show <id>`, `export <id>`

## 기본 경로
- 데이터: `./data`
- 시드: `./seeds`

## Fuzz Host 준비(의존성 설치)

새 퍼징 PC/WSL에서는 `git clone`만으로 시스템 의존성이 설치되지 않는다.
아래 스크립트로 호스트 의존성을 먼저 맞춘다.

```bash
bash scripts/setup_fuzz_host.sh
# docker 제외 시
bash scripts/setup_fuzz_host.sh --no-docker
```

설치 대상(기본):
- `rustup/cargo` (프로젝트 빌드)
- `clang` (libFuzzer 경로)
- `docker.io` (AFL++ Docker 경로, `--no-docker`로 제외 가능)
- `build-essential`, `pkg-config`
- `curl`, `git`, `jq`, `tmux`, `python3`, `python3-pip`

설치 후 확인:

```bash
docker --version
docker run --rm aflplusplus/aflplusplus afl-fuzz -h >/dev/null; echo "EC=$?"
clang++ --version
```

## 엔진 실환경 검증(ONNX 기준)

`tool run --backend <backend>`는 backend 1개만 선택해 실행한다. 장시간 비교/헌팅은 `tool campaign --mode <serial|parallel>`로 묶어 실행한다.

- `serial`: 같은 시드 스냅샷을 기준으로 backend를 순서대로 실행한다. 논문식 비교/재현성 기준에 적합하다.
- `parallel`: 같은 시드 스냅샷을 각 backend가 읽고, 결과는 backend별 data dir에 분리해 동시에 실행한다. 버그 헌팅 시간 단축 목적에 적합하다.

```bash
tool campaign --mode serial --target onnx --hours 168 --campaign-id paper-onnx-001
tool campaign --mode parallel --target onnx --hours 168 --campaign-id hunt-onnx-001
```

짧은 스모크는 시간/입력 수를 제한한다.

```bash
make smoke TARGET=onnx
make smoke-all TARGET=onnx SECONDS=60
```

캠페인 결과는 `data/campaigns/<campaign-id>/` 아래에 저장된다. 원본 시드는 `seeds/<target>/`에서 캠페인 스냅샷으로 한 번 복사되고, backend별 실행은 `arms/<backend>/corpus/`와 `arms/<backend>/data/`로 분리된다. 같은 `campaign-id` 재사용은 결과 혼합을 막기 위해 차단한다. 중단 신호(`Ctrl+C`, `TERM`)를 받으면 실행 중인 backend 프로세스를 종료하고 `status.json`/`arms/<backend>/status.json`에 `interrupted` 상태를 기록한다.

퍼징컴에서는 먼저 `make build`, `make preflight TARGET=onnx`, `make smoke TARGET=onnx` 순서로 확인한다.

### AFL++
```bash
TOOL_AFLPP_CMD='docker run --rm {docker_user_flag} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc "afl-fuzz -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- /bin/true @@ >/dev/null 2>&1"' \
cargo run --offline -- run --target onnx --backend aflpp --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1
```

### AFL++ (tool harness 연결)
```bash
TOOL_AFLPP_CMD='docker run --rm {docker_user_flag} {docker_hardening_flags} {docker_readonly_flags} -v {workdir_abs}:/work:ro -v {corpus_dir_abs}:/corpus:ro -v {run_dir_abs}:/out -w /work aflplusplus/aflplusplus bash -lc "afl-fuzz -n -V 5 -i {container_corpus_dir} -o {container_run_dir}/afl-out -- /work/target/debug/tool harness --target onnx --input @@ >/dev/null 2>&1"' \
cargo run --offline -- run --target onnx --backend aflpp --corpus-dir seeds/onnx --workers 1 --timeout-sec 30 --restart-limit 1
```
`permission denied ... docker.sock`가 나오면 Docker 그룹 권한을 다시 적용(`newgrp docker`)하거나 새 셸에서 재시도한다. `run --backend aflpp`는 `{docker_user_flag}`를 현재 사용자로 치환해 `afl-out`이 root 소유로 남지 않게 한다.

### libFuzzer (경로 스모크)
```bash
TOOL_LIBFUZZER_CMD='clang++ --version >/dev/null' \
cargo run --offline -- run --target onnx --backend libfuzzer --corpus-dir seeds/onnx --workers 2 --timeout-sec 30 --restart-limit 1
```

### libFuzzer (tool harness 연결)
```bash
scripts/build_libfuzzer_tool_driver.sh
TOOL_LIBFUZZER_CMD='mkdir -p {artifact_dir} && TOOL_HARNESS_TOOL=./target/debug/tool TOOL_HARNESS_TARGET=onnx TOOL_HARNESS_EXT=onnx ./harnesses/libfuzzer/tool_harness_driver -artifact_prefix={artifact_dir}/ -max_total_time=5 {corpus_dir} >/dev/null 2>&1' \
cargo run --offline -- run --target onnx --backend libfuzzer --corpus-dir seeds/onnx --workers 1 --timeout-sec 30 --restart-limit 1
```

결과 확인:
```bash
LATEST=$(ls -dt data/runs/run-* | head -n 1)
cat "$LATEST/status.json"
ls -la "$LATEST/logs"
```
