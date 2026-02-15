# 개발 TODO (v1.0)

## 1) 스캐폴딩
- [x] CLI 골격 생성 (run/triage/report, list/show/export)
- [x] 기본 설정/경로 로딩 (`./data`, `./seeds`)

## 2) 타깃 다운로드/메타
- [x] 공식 배포본 다운로드 규칙 적용
- [x] 버전 고정 + 해시 기록
- [x] 메타 저장 포맷 정의

## 3) 하네스 통합
- [x] 공통 하네스 라우팅 + 포맷 프리체크 구현 (`tool harness`)
- [x] 직접 연결 재시도 경로 추가 (onnxruntime/safetensors Python probe, llama.cpp probe)
- [ ] GGUF 하네스: llama.cpp 파서 연결
- [ ] ONNX 하네스: onnxruntime 연결
- [ ] safetensors 하네스: 공식 라이브러리 연결

## 4) 퍼징 실행
- [x] 퍼저 실행 파이프라인
- [x] 병렬 실행 기본값 8 적용
- [x] 리소스 제한/재시작 정책

## 5) 재현/검증
- [x] 3회 재현 규칙 적용
- [x] 스택 상위 3프레임 비교
- [x] 실패 모드(플레이키/타임아웃) 처리

## 6) 리포트/보관
- [ ] 리포트 자동 생성
- [ ] 보관 정책 적용 (30일, 로그 zstd, core dump OFF)
- [x] 리포트 샘플 1개 작성

## 7) 운영 지표
- [ ] 커버리지/크래시/유효율 수집 정의
