# Bug Bounty Fuzzing Platform (v2 Renewal)

> "퍼징으로 찾았다"에서 끝내지 않고, **재현·검증·리포트까지 자동화**하는 버그바운티용 퍼징 플랫폼.

## 먼저 읽기
- 설계/결정: [first.md](first.md)
- 구현 명세: [docs/specs.md](docs/specs.md)

## 기존 툴 대비 차별점 (Differentiators)
- **Deep & Structured Fuzzing**: 구조 인지형 mutator/harness로 얕은 파싱 에러가 아니라 깊은 경로의 메모리 오염을 겨냥한다.
- **Auto-Verification**: 동일 컨테이너에서 3회 재현 검증하고, 증거 번들/리포트를 자동 생성해 제출 품질을 보장한다.
- **Exploitability Triage**: ASan/Release 교차 검증과 스택/레지스터 분석으로 RCE 가능성을 등급화한다.
- **Reproducibility by Design**: 환경 고정/해시 기록으로 재현성을 강화한다.
- **LLM Assist (Out of Loop)**: 퍼징 루프 외부에서 Seed/Dictionary/Mutation guide를 보조한다.

### 목표 (Goals)
- 구조 인지형 mutator/harness로 더 깊은 경로를 타겟한다.
- 기존 툴 대비 재현 성공률/제출 승인률을 수치로 개선한다.

## RCE 탐지 방법론 (요약)
- 핵심 본체는 **하네스/뮤테이터/triage**이며, 자세한 정책은 [first.md](first.md)와 [docs/specs.md](docs/specs.md)에 정리한다.
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
- 설계/결정: [first.md](first.md)
- 구현 명세: [docs/specs.md](docs/specs.md)
- 협업 규칙: [docs/rules.md](docs/rules.md)
- 문서 TODO: [docs/todo.md](docs/todo.md)
- 개발 로드맵: [docs/roadmap.md](docs/roadmap.md)
- 개발 TODO: [docs/dev-todo.md](docs/dev-todo.md)
- 리포트 샘플: [docs/report-sample.md](docs/report-sample.md)

## CLI (확정)
- `tool run`, `tool triage`, `tool report`
- 결과 조회: `list`, `show <id>`, `export <id>`

## 기본 경로
- 데이터: `./data`
- 시드: `./seeds`
