# Bug Bounty Fuzzing Platform (v1.0)

## 개요
퍼징 크래시를 **재현·검증·리포트**까지 자동화하는 버그바운티용 퍼징 플랫폼.

## 핵심 목표
- 대상 포맷: **GGUF / ONNX / safetensors**
- 유효 버그 기준: **SEGV/Abort + 동일 입력 3회 재현 + 상위 3프레임 동일**
- CLI 우선, 대시보드는 이후 단계

## 문서
- 로드맵/결정: [first.md](first.md)
- 협업 규칙: [docs/rules.md](docs/rules.md)
- 문서 TODO: [docs/todo.md](docs/todo.md)
- 개발 로드맵: [docs/roadmap.md](docs/roadmap.md)
- 개발 TODO: [docs/dev-todo.md](docs/dev-todo.md)
- 리포트 샘플: [docs/report-sample.md](docs/report-sample.md)

## CLI (계획)
- `tool run`, `tool triage`, `tool report`
- 결과 조회: `list`, `show <id>`, `export <id>`

## 기본 경로
- 데이터: `./data`
- 시드: `./seeds`
