#!/usr/bin/env bash
# Build a coverage-instrumented onnxruntime (CPU, shared lib) for the V2 ONNX
# real-coverage PoC.
#
# Instrumentation: LLVM source-based coverage (-fprofile-instr-generate
# -fcoverage-mapping). The flags are injected GLOBALLY via CMAKE_*_FLAGS so the
# whole libonnxruntime.so is instrumented -- not just a thin driver (the failure
# mode of the existing black-box libfuzzer harness, which only instruments ~311
# driver edges and runs onnxruntime in a separate, uninstrumented process).
#
# Output: an instrumented libonnxruntime.so under $BUILD_DIR/$CONFIG.
# Heavy step (~1-2h). Optional/experimental; NOT part of run -> triage -> report.
#
# Requires: clang/clang++ (14), CMake >= 3.28 (<4). The system CMake 3.22 is too
# old for onnxruntime 1.23.2 (cmake_minimum_required 3.28); install a 3.x build
# with:  .venv/bin/pip install "cmake>=3.28,<4"
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-/home/ssw/bugbounty}"
ORT_VER="${ORT_VER:-v1.23.2}"
ORT_SRC="${ORT_SRC:-$PROJECT_ROOT/data/targets/onnxruntime/$ORT_VER/onnxruntime-1.23.2}"
BUILD_DIR="${BUILD_DIR:-build/cov}"          # relative to ORT_SRC (build.py convention)
CONFIG="${CONFIG:-RelWithDebInfo}"
PARALLEL="${PARALLEL:-6}"                     # modest: onnxruntime C++ TUs are RAM-heavy on 15 GiB
CMAKE="${CMAKE:-$PROJECT_ROOT/.venv/bin/cmake}"
# Toolchain: onnxruntime 1.23.2 MLAS ships AVX-NE-CONVERT asm (vcvtneeph2ps) that
# the system clang 14 / gcc 11 / binutils 2.38 cannot assemble. Use a newer LLVM
# (>=16). Default to the locally unpacked clang-17 (no sudo, under data/toolchains).
CLANG_DIR="${CLANG_DIR:-$PROJECT_ROOT/data/toolchains/clang+llvm-17.0.6-x86_64-linux-gnu-ubuntu-22.04}"
CLANG="${CLANG:-$CLANG_DIR/bin/clang}"
CLANGXX="${CLANGXX:-$CLANG_DIR/bin/clang++}"
COV_FLAGS="-fprofile-instr-generate -fcoverage-mapping"

echo "[build-cov-onnx] src=$ORT_SRC"
echo "[build-cov-onnx] config=$CONFIG parallel=$PARALLEL cmake=$("$CMAKE" --version | head -1)"
echo "[build-cov-onnx] clang=$("$CLANG" --version | head -1)"
[ -d "$ORT_SRC" ] || { echo "[build-cov-onnx] ORT source not found: $ORT_SRC (extract the release tarball first)"; exit 1; }
[ -x "$CLANG" ]   || { echo "[build-cov-onnx] clang not found: $CLANG";     exit 1; }
[ -x "$CLANGXX" ] || { echo "[build-cov-onnx] clang++ not found: $CLANGXX"; exit 1; }

cd "$ORT_SRC"
export PATH="$CLANG_DIR/bin:$PATH"
export CC="$CLANG" CXX="$CLANGXX"

# Toolchain-compat patch: clang < 18 doesn't know the "waitpkg" feature string for
# __builtin_cpu_supports (onnxruntime 1.23.2 cpuid_info.cc:163). Read the CPUID bit
# directly instead -- identical to the file's own non-Linux branch. Idempotent.
CPUID_FILE="$ORT_SRC/onnxruntime/core/common/cpuid_info.cc"
if grep -q '__builtin_cpu_supports("waitpkg")' "$CPUID_FILE"; then
  sed -i 's|has_tpause_ = __builtin_cpu_supports("waitpkg") != 0;|has_tpause_ = (data[2] \& (1 << 5)) != 0;  // patched: clang<18 lacks "waitpkg" __builtin_cpu_supports|' "$CPUID_FILE"
  echo "[build-cov-onnx] patched cpuid_info.cc (waitpkg builtin -> CPUID bit)"
fi
# clang < 18: __builtin_ia32_tpause expects 3 args (control, hi, lo); onnxruntime
# calls it with a 64-bit counter (2 args). TPAUSE is only a spin-loop power hint, so
# fall back to the standard _mm_pause() (same as the file's own #else branch). Idempotent.
SPIN_FILE="$ORT_SRC/onnxruntime/core/common/spin_pause.cc"
if grep -q '__builtin_ia32_tpause(0x0' "$SPIN_FILE"; then
  sed -i 's|__builtin_ia32_tpause(0x0, __rdtsc() + tpause_spin_delay_cycles);|_mm_pause();  // patched: clang<18 __builtin_ia32_tpause arity differs; TPAUSE here is only a spin hint|' "$SPIN_FILE"
  echo "[build-cov-onnx] patched spin_pause.cc (tpause builtin -> _mm_pause)"
fi

python3 tools/ci_build/build.py \
  --build_dir "$BUILD_DIR" \
  --config "$CONFIG" \
  --build_shared_lib \
  --skip_tests \
  --compile_no_warning_as_error \
  --cmake_path "$CMAKE" \
  --update --build --parallel "$PARALLEL" \
  --cmake_extra_defines \
    onnxruntime_BUILD_UNIT_TESTS=OFF \
    "CMAKE_C_COMPILER=$CLANG" "CMAKE_CXX_COMPILER=$CLANGXX" \
    "CMAKE_C_FLAGS=$COV_FLAGS" \
    "CMAKE_CXX_FLAGS=$COV_FLAGS"

SO="$ORT_SRC/$BUILD_DIR/$CONFIG/libonnxruntime.so"
echo "[build-cov-onnx] done."
if [ -f "$SO" ]; then
  ls -la "$SO"
  echo "[build-cov-onnx] instrumented lib: $SO"
else
  echo "[build-cov-onnx] WARNING: expected .so not found at $SO"
  exit 1
fi
