#!/usr/bin/env bash
set -euo pipefail

WORKDIR="${WORKDIR:-$PWD}"
PROJECT_ROOT="${PROJECT_ROOT:-$WORKDIR}"
ORT_VER="${ORT_VER:-v1.23.2}"
ORT_SRC="${ORT_SRC:-$PROJECT_ROOT/data/targets/onnxruntime/$ORT_VER/onnxruntime-1.23.2}"
CONFIG="${CONFIG:-RelWithDebInfo}"
SRC="${SRC:-$PROJECT_ROOT/harnesses/libfuzzer/onnxruntime_loader_fuzzer.cc}"
OUT="${OUT:-$PROJECT_ROOT/harnesses/aflpp/onnxruntime_loader_replay}"
AFL_CXX="${AFL_CXX:-afl-clang-fast++}"
COVERAGE_FLAGS="${COVERAGE_FLAGS:-}"
EXTRA_CXXFLAGS="${AFLPP_EXTRA_CXXFLAGS:-}"

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
  echo "[build-aflpp-onnx-native] libonnxruntime.so not found under $ORT_SRC/build" >&2
  echo "[build-aflpp-onnx-native] set SO_DIR or build ONNX Runtime first" >&2
  exit 1
fi

SO="$SO_DIR/libonnxruntime.so"
INCLUDE_DIR="$ORT_SRC/include/onnxruntime/core/session"

if ! command -v "$AFL_CXX" >/dev/null 2>&1 && [[ ! -x "$AFL_CXX" ]]; then
  echo "[build-aflpp-onnx-native] compiler not found: $AFL_CXX" >&2
  echo "[build-aflpp-onnx-native] run inside aflplusplus/aflplusplus or set AFL_CXX" >&2
  exit 1
fi

[[ -f "$SRC" ]] || { echo "[build-aflpp-onnx-native] source not found: $SRC" >&2; exit 1; }
[[ -f "$SO" ]] || { echo "[build-aflpp-onnx-native] shared library not found: $SO" >&2; exit 1; }
[[ -f "$INCLUDE_DIR/onnxruntime_cxx_api.h" ]] || { echo "[build-aflpp-onnx-native] header not found: $INCLUDE_DIR/onnxruntime_cxx_api.h" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"

echo "[build-aflpp-onnx-native] compiling"
"$AFL_CXX" -std=c++17 -O1 -g \
  -DONNX_FUZZ_STANDALONE \
  $COVERAGE_FLAGS \
  $EXTRA_CXXFLAGS \
  -I"$INCLUDE_DIR" \
  "$SRC" \
  -L"$SO_DIR" -lonnxruntime -Wl,-rpath,"$SO_DIR" \
  -o "$OUT"

echo "[build-aflpp-onnx-native] done"
echo "src: $SRC"
echo "out: $OUT"
echo "so: $SO"
