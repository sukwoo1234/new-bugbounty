#!/usr/bin/env bash
set -euo pipefail

WORKDIR="${WORKDIR:-$PWD}"
SRC="${SRC:-$WORKDIR/harnesses/libfuzzer/tool_harness_driver.cc}"
OUT="${OUT:-$WORKDIR/harnesses/libfuzzer/tool_harness_driver}"

if ! command -v clang++ >/dev/null 2>&1; then
  echo "[build-libfuzzer-driver] clang++ not found"
  exit 1
fi

mkdir -p "$(dirname "$OUT")"

echo "[build-libfuzzer-driver] compiling"
clang++ -O1 -g -std=c++17 -fsanitize=fuzzer "$SRC" -o "$OUT"

echo "[build-libfuzzer-driver] done"
echo "src: $SRC"
echo "out: $OUT"
