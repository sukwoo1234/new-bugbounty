#!/usr/bin/env bash
set -euo pipefail

WORKDIR="${WORKDIR:-$PWD}"
PROJECT_ROOT="${PROJECT_ROOT:-$WORKDIR}"
ORT_VER="${ORT_VER:-v1.23.2}"
ORT_SRC="${ORT_SRC:-$PROJECT_ROOT/data/targets/onnxruntime/$ORT_VER/onnxruntime-1.23.2}"
CONFIG="${CONFIG:-RelWithDebInfo}"
SRC="${SRC:-$PROJECT_ROOT/harnesses/libfuzzer/onnxruntime_loader_fuzzer.cc}"
OUT="${OUT:-$PROJECT_ROOT/harnesses/libfuzzer/onnxruntime_loader_fuzzer}"
FUZZ_SANITIZERS="${FUZZ_SANITIZERS:-fuzzer}"
COVERAGE_FLAGS="${COVERAGE_FLAGS:-}"
CLANG_BUNDLE="$PROJECT_ROOT/data/toolchains/clang+llvm-17.0.6-x86_64-linux-gnu-ubuntu-22.04/bin/clang++"

if [[ -z "${CLANGXX:-}" ]]; then
  if [[ -x "$CLANG_BUNDLE" ]]; then
    CLANGXX="$CLANG_BUNDLE"
  else
    CLANGXX="clang++"
  fi
fi

if [[ -z "${SO_DIR:-}" ]]; then
  for candidate in \
    "$ORT_SRC/build/Linux/Release" \
    "$ORT_SRC/build/Linux/$CONFIG" \
    "$ORT_SRC/build/cov-o0/$CONFIG" \
    "$ORT_SRC/build/cov/$CONFIG"
  do
    if [[ -f "$candidate/libonnxruntime.so" ]]; then
      SO_DIR="$candidate"
      break
    fi
  done
fi

if [[ -z "${SO_DIR:-}" ]]; then
  echo "[build-libfuzzer-onnx-native] libonnxruntime.so not found under $ORT_SRC/build" >&2
  echo "[build-libfuzzer-onnx-native] set SO_DIR or build ONNX Runtime first" >&2
  exit 1
fi

SO="$SO_DIR/libonnxruntime.so"
INCLUDE_DIR="$ORT_SRC/include/onnxruntime/core/session"

if ! command -v "$CLANGXX" >/dev/null 2>&1 && [[ ! -x "$CLANGXX" ]]; then
  echo "[build-libfuzzer-onnx-native] clang++ not found: $CLANGXX" >&2
  exit 1
fi

[[ -f "$SRC" ]] || { echo "[build-libfuzzer-onnx-native] source not found: $SRC" >&2; exit 1; }
[[ -f "$SO" ]] || { echo "[build-libfuzzer-onnx-native] shared library not found: $SO" >&2; exit 1; }
[[ -f "$INCLUDE_DIR/onnxruntime_cxx_api.h" ]] || { echo "[build-libfuzzer-onnx-native] header not found: $INCLUDE_DIR/onnxruntime_cxx_api.h" >&2; exit 1; }

if [[ "$SO_DIR" == *"/build/cov/"* || "$SO_DIR" == *"/build/cov-o0/"* ]]; then
  COVERAGE_FLAGS="${COVERAGE_FLAGS:--fprofile-instr-generate -fcoverage-mapping}"
fi

mkdir -p "$(dirname "$OUT")"

echo "[build-libfuzzer-onnx-native] compiling"
"$CLANGXX" -std=c++17 -O1 -g \
  -fsanitize="$FUZZ_SANITIZERS" \
  $COVERAGE_FLAGS \
  -I"$INCLUDE_DIR" \
  "$SRC" \
  -L"$SO_DIR" -lonnxruntime -Wl,-rpath,"$SO_DIR" \
  -o "$OUT"

echo "[build-libfuzzer-onnx-native] done"
echo "src: $SRC"
echo "out: $OUT"
echo "so: $SO"
