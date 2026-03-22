# 유효 코퍼스 준비 SOP (v1.0+)

## 목적
- `prepare-target`으로 고정한 공식 타깃 버전 기준으로, 실제에 가까운 유효 seed corpus를 준비한다.
- 퍼징 시작 전에 parser/loader 경로에 진입 가능한 입력만 남겨 탐색 효율을 높인다.

## 핵심 원칙
- seed는 "입력 파일"을 뜻한다.
- `prepare-target`은 타깃 고정 단계이고, corpus 준비는 별도 단계다.
- 정상 샘플 중심으로 시작하고, 준손상 샘플은 보조로만 사용한다.

## 폴더 구조 (권장)
```text
seeds/
  gguf/
  onnx/
  safetensors/
```

## 단계
1. 타깃 고정
- `tool prepare-target --target gguf`
- `tool prepare-target --target onnx`
- `tool prepare-target --target safetensors`
- `data/targets/<target>/<version>/meta.json` 생성 여부 확인

2. 원본 샘플 수집(공식/공개 배포본)
- GGUF: 공개 배포 GGUF(라이선스/배포 조건 확인)
- ONNX: 공식/공개 ONNX 모델
- safetensors: 공식/공개 safetensors 파일
- 최소 권장: 포맷당 20개 이상

3. 사전 필터링(유효 seed 선별)
- `tool harness --target <format> --input <file>` 실행
- 아래 조건을 통과한 파일만 seed로 채택
  - `parser_step` 성공
  - `library_step`이 unavailable이 아님

4. seed 배치
- 선별 통과 파일을 `seeds/<format>/`에 복사
- 중복 파일은 SHA-256 기준 제거

5. 퍼징 시작
- `tool run --target <format> --corpus-dir seeds/<format> --workers <n> --timeout-sec <t>`
- 이후 `tool triage` -> `tool report` 순서로 검증/리포트 생성

## 체크리스트
- [ ] `prepare-target` 3타깃 완료 + `meta.json` 확인
- [ ] 포맷당 seed 20개 이상 확보
- [ ] `harness` 사전검증 통과율 기록
- [ ] seed 중복 제거 완료
- [ ] `run/triage/report` 1회 스모크 성공

## 운영 메모
- 장시간 퍼징 효과는 seed 품질에 크게 좌우된다.
- 코퍼스가 너무 작거나 깨진 샘플 위주면 얕은 경로만 반복될 수 있다.
