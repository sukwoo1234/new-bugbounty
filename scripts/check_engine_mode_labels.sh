#!/usr/bin/env bash
# G2/G4: the engine loops must never fall back to a black-box run silently.
# Verifies, with fake binaries only (no docker, no AFL++, no libFuzzer needed):
#   - a missing native libFuzzer driver logs a WARN and labels the run blackbox
#   - a present native driver labels the run native
#   - a non-instrumented AFL++ replay binary logs a WARN and labels the run blackbox_n
#   - an instrumented AFL++ replay binary labels the run instrumented
#   - REQUIRE_NATIVE / REQUIRE_INSTRUMENTED turn the fallback into a hard failure
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

log() {
  echo "[engine-mode-check] $*"
}

fail() {
  echo "[engine-mode-check] fail: $*" >&2
  exit 1
}

mkdir -p "$WORK/bin" "$WORK/seeds/onnx" "$WORK/harnesses/libfuzzer" "$WORK/harnesses/aflpp"
printf 'seed' > "$WORK/seeds/onnx/seed.onnx"

# stand-in for target/release/tool: the loops only need one iteration to complete
cat > "$WORK/bin/tool" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$WORK/bin/tool"

run_loop() {
  local script="$1"
  shift
  set +e
  env -u REQUIRE_NATIVE -u REQUIRE_INSTRUMENTED -u LIBFUZZER_DRIVER -u AFLPP_CONTAINER_TOOL \
    -u RUNS_ROOT -u AFLPP_RUN_SUMMARY_ROOT -u AFLPP_MAX_RUN_DIRS_KEEP -u TOOL_AFLPP_CMD \
    -u TOOL_LIBFUZZER_CMD -u WORKERS -u TIMEOUT_SEC -u RESTART_LIMIT \
    PROJECT_ROOT="$WORK" \
    TARGET=onnx \
    TOOL_BIN="$WORK/bin/tool" \
    CORPUS_DIR="$WORK/seeds/onnx" \
    FUZZ_LOOP_MAX_ITERATIONS=1 \
    ITERATION_SLEEP_SEC=0 \
    "$@" \
    bash "$PROJECT_ROOT/ops/scripts/$script" >"$WORK/out.log" 2>&1
  LOOP_EXIT=$?
  set -e
  LOOP_OUT="$(cat "$WORK/out.log")"
}

assert_contains() {
  case "$LOOP_OUT" in
    *"$1"*) ;;
    *) fail "expected output to contain '$1'; got:\n$LOOP_OUT" ;;
  esac
}

assert_not_contains() {
  case "$LOOP_OUT" in
    *"$1"*) fail "expected output NOT to contain '$1'; got:\n$LOOP_OUT" ;;
    *) ;;
  esac
}

# The Environment= lines a shipped systemd unit hands the loop, as VAR=VALUE arguments.
unit_env() {
  sed -n 's/^Environment=//p' "$1"
}

# --- instrumentation_scope (B1) -------------------------------------------------
# G2 again, one level deeper. has_afl_instrumentation() answers "does this binary
# carry the forkserver", which is NOT "is the parser instrumented": a driver that is
# instrumented but reaches its parser through a separate .so gives driver-level
# coverage only. That is exactly what the ONNX arm was labelled "instrumented" for.
# The fixtures are real ELF binaries with the AFL marker appended, because the
# decision turns on DEFINED symbols and a shell script has none.
. "$PROJECT_ROOT/scripts/lib/engine_mode.sh"

afl_marked_copy() {
  # afl_marked_copy <src> <dst>: a real binary that also looks AFL-instrumented.
  cp "$1" "$2"
  printf '__AFL_SHM_ID __afl_area_ptr' >>"$2"
  chmod +x "$2"
}

# has_afl_instrumentation() decides whether the AFL++ arm runs instrumented or
# black-box, and its nm branch matches symbols (__afl_prev_loc, __afl_shm, __afl_fuzz)
# that the raw-bytes fallback below it does NOT list. All three callers run under
# `set -o pipefail`, so if that branch loses its answer to a SIGPIPE the whole arm
# silently drops to blackbox_n - G2 again. This fixture is a real binary carrying only
# __afl_prev_loc, padded so nm's output is far larger than a pipe buffer.
SCOPE_CC="${SCOPE_CC:-$PROJECT_ROOT/data/toolchains/clang+llvm-17.0.6-x86_64-linux-gnu-ubuntu-22.04/bin/clang}"
[ -x "$SCOPE_CC" ] || SCOPE_CC="$(command -v cc || command -v gcc || command -v clang || true)"
if [ -n "$SCOPE_CC" ] && [ -x "$SCOPE_CC" ]; then
  log "has_afl_instrumentation: a large symbol table must not lose the answer to SIGPIPE"
  {
    printf 'void *__afl_prev_loc;\n'
    awk 'BEGIN { for (i = 0; i < 4000; i++) printf "long pad_symbol_%d = %d;\n", i, i }'
    printf 'int main(void) { return 0; }\n'
  } > "$WORK/afl_symbols.c"
  if "$SCOPE_CC" -O0 "$WORK/afl_symbols.c" -o "$WORK/harnesses/aflpp/big_symtab" 2>/dev/null; then
    # The fallback list must NOT cover this binary, or the case proves nothing.
    if grep -qaE '__AFL_SHM_ID|__AFL_SHM_FUZZ_ID|__afl_area_initial|__afl_area_ptr' \
        "$WORK/harnesses/aflpp/big_symtab"; then
      fail "fixture is covered by the raw-bytes fallback; it cannot exercise the nm branch"
    fi
    has_afl_instrumentation "$WORK/harnesses/aflpp/big_symtab" \
      || fail "has_afl_instrumentation lost __afl_prev_loc on a large symbol table"
  else
    log "skip SIGPIPE case: $SCOPE_CC could not build the fixture"
  fi
else
  log "skip SIGPIPE case: no C compiler available"
fi

log "instrumentation_scope: a binary without the forkserver is none"
cp /bin/true "$WORK/harnesses/aflpp/plain_bin"
scope="$(instrumentation_scope "$WORK/harnesses/aflpp/plain_bin")"
[ "$scope" = "none" ] || fail "expected none, got '$scope'"

log "instrumentation_scope: a driver whose parser is not linked in is driver_only"
afl_marked_copy /bin/true "$WORK/harnesses/aflpp/dyn_replay"
scope="$(instrumentation_scope "$WORK/harnesses/aflpp/dyn_replay")"
[ "$scope" = "driver_only" ] || fail "expected driver_only, got '$scope'"

log "instrumentation_scope: a missing binary is none, not a crash"
scope="$(instrumentation_scope "$WORK/harnesses/aflpp/does-not-exist")"
[ "$scope" = "none" ] || fail "expected none for a missing binary, got '$scope'"

# The gguf harness links ggml statically (BUILD_SHARED_LIBS=OFF), so the parser really
# is inside the binary - that is what makes library-wide scope claimable for gguf and
# not for onnx.
GGUF_REPLAY="$PROJECT_ROOT/harnesses/libfuzzer/gguf_loader_replay"
if [ -x "$GGUF_REPLAY" ]; then
  log "instrumentation_scope: a statically linked parser is library scope"
  afl_marked_copy "$GGUF_REPLAY" "$WORK/harnesses/aflpp/static_replay"
  scope="$(instrumentation_scope "$WORK/harnesses/aflpp/static_replay")"
  [ "$scope" = "library" ] || fail "expected library, got '$scope'"
else
  log "skip library-scope case: no gguf replay at $GGUF_REPLAY (build it first)"
fi

# The shipped ONNX drivers must NOT claim library scope: onnxruntime is a .so.
for onnx_driver in \
  "$PROJECT_ROOT/harnesses/libfuzzer/onnxruntime_loader_fuzzer" \
  "$PROJECT_ROOT/harnesses/aflpp/onnxruntime_loader_replay"
do
  [ -x "$onnx_driver" ] || continue
  log "instrumentation_scope: $(basename "$onnx_driver") must not claim library scope"
  afl_marked_copy "$onnx_driver" "$WORK/harnesses/aflpp/onnx_scope_probe"
  scope="$(instrumentation_scope "$WORK/harnesses/aflpp/onnx_scope_probe")"
  [ "$scope" = "driver_only" ] || fail "expected driver_only for $onnx_driver, got '$scope'"
done

log "libfuzzer: missing native driver must warn and label blackbox"
cp "$WORK/bin/tool" "$WORK/harnesses/libfuzzer/tool_harness_driver"
rm -f "$WORK/harnesses/libfuzzer/onnxruntime_loader_fuzzer"
run_loop fuzz-loop-libfuzzer.sh
[ "$LOOP_EXIT" -eq 0 ] || fail "libfuzzer blackbox loop exited $LOOP_EXIT"
assert_contains "WARN"
assert_contains "libfuzzer_mode=blackbox"

log "libfuzzer: REQUIRE_NATIVE=1 must fail instead of falling back"
run_loop fuzz-loop-libfuzzer.sh REQUIRE_NATIVE=1
[ "$LOOP_EXIT" -ne 0 ] || fail "REQUIRE_NATIVE=1 must exit non-zero when the native driver is missing"

log "libfuzzer: present native driver must label native"
cp "$WORK/bin/tool" "$WORK/harnesses/libfuzzer/onnxruntime_loader_fuzzer"
run_loop fuzz-loop-libfuzzer.sh
[ "$LOOP_EXIT" -eq 0 ] || fail "libfuzzer native loop exited $LOOP_EXIT"
assert_contains "libfuzzer_mode=native"

# B2: the native-driver decision was hardcoded to onnx, so a gguf run with a perfectly
# good native driver sitting right there was labelled blackbox and ran through the
# black-box tool wrapper - a campaign that looks healthy and fuzzes the wrong thing.
mkdir -p "$WORK/seeds/gguf"
printf 'GGUF' > "$WORK/seeds/gguf/seed.gguf"

log "libfuzzer: a missing native gguf driver must still warn and label blackbox"
rm -f "$WORK/harnesses/libfuzzer/gguf_loader_fuzzer"
run_loop fuzz-loop-libfuzzer.sh TARGET=gguf CORPUS_DIR="$WORK/seeds/gguf"
[ "$LOOP_EXIT" -eq 0 ] || fail "libfuzzer gguf blackbox loop exited $LOOP_EXIT"
assert_contains "WARN"
assert_contains "libfuzzer_mode=blackbox"
assert_contains "gguf_loader_fuzzer"

log "libfuzzer: a native gguf driver must label native, not blackbox"
cp "$WORK/bin/tool" "$WORK/harnesses/libfuzzer/gguf_loader_fuzzer"
run_loop fuzz-loop-libfuzzer.sh TARGET=gguf CORPUS_DIR="$WORK/seeds/gguf"
[ "$LOOP_EXIT" -eq 0 ] || fail "libfuzzer gguf native loop exited $LOOP_EXIT"
assert_contains "libfuzzer_mode=native"
assert_contains "gguf_loader_fuzzer"
# the profile file name must follow the target, or two arms overwrite each other
assert_not_contains "onnx-native-%p.profraw"

log "libfuzzer: an unsupported target has no native driver and says so"
run_loop fuzz-loop-libfuzzer.sh TARGET=safetensors CORPUS_DIR="$WORK/seeds/gguf"
[ "$LOOP_EXIT" -eq 0 ] || fail "libfuzzer safetensors loop exited $LOOP_EXIT"
assert_contains "libfuzzer_mode=blackbox"

log "aflpp: non-instrumented replay binary must warn and label blackbox_n"
cp "$WORK/bin/tool" "$WORK/harnesses/aflpp/onnxruntime_loader_replay"
run_loop fuzz-loop-aflpp.sh
[ "$LOOP_EXIT" -eq 0 ] || fail "aflpp blackbox loop exited $LOOP_EXIT"
assert_contains "WARN"
assert_contains "aflpp_mode=blackbox_n"
assert_contains "afl-fuzz -n "
# G3: in black-box mode AFL++ only sees signals, so a library crash (tool exit 4) is
# invisible unless the exit code is declared a crash.
assert_contains "AFL_CRASH_EXITCODE=4"
assert_contains "AFL_IGNORE_SEED_PROBLEMS=1"

log "aflpp: REQUIRE_INSTRUMENTED=1 must fail instead of falling back"
run_loop fuzz-loop-aflpp.sh REQUIRE_INSTRUMENTED=1
[ "$LOOP_EXIT" -ne 0 ] || fail "REQUIRE_INSTRUMENTED=1 must exit non-zero without instrumentation"

log "aflpp: sancov-only binary is NOT AFL++ instrumentation"
{
  printf '#!/usr/bin/env bash\nexit 0\n'
  printf '# __sanitizer_cov_trace_pc_guard __sanitizer_cov_trace_cmp1\n'
} > "$WORK/harnesses/aflpp/onnxruntime_loader_replay"
chmod +x "$WORK/harnesses/aflpp/onnxruntime_loader_replay"
run_loop fuzz-loop-aflpp.sh
[ "$LOOP_EXIT" -eq 0 ] || fail "aflpp sancov-only loop exited $LOOP_EXIT"
assert_contains "aflpp_mode=blackbox_n"

log "aflpp: instrumented replay binary must label instrumented and drop -n"
{
  printf '#!/usr/bin/env bash\nexit 0\n'
  printf '# __AFL_SHM_ID __afl_area_ptr\n'
} > "$WORK/harnesses/aflpp/onnxruntime_loader_replay"
chmod +x "$WORK/harnesses/aflpp/onnxruntime_loader_replay"
run_loop fuzz-loop-aflpp.sh
[ "$LOOP_EXIT" -eq 0 ] || fail "aflpp instrumented loop exited $LOOP_EXIT"
assert_contains "aflpp_mode=instrumented"
assert_not_contains "afl-fuzz -n "

# The loop treats an operator-set AFLPP_CONTAINER_TOOL as a deliberate override and will
# not replace it with the instrumented replay. A shipped unit that sets it - even to the
# value the loop already defaults to - therefore pins the arm to blackbox_n forever.
log "aflpp: the shipped systemd unit must not disable the native path"
# shellcheck disable=SC2046
run_loop fuzz-loop-aflpp.sh $(unit_env "$PROJECT_ROOT/ops/systemd/tool-fuzz-onnx-aflpp.service")
[ "$LOOP_EXIT" -eq 0 ] || fail "aflpp systemd-env loop exited $LOOP_EXIT"
assert_contains "aflpp_mode=instrumented"
assert_not_contains "afl-fuzz -n "

# scripts/run_long.sh is the path the campaign runners take (run_campaign.sh /
# run_onnx_abc_week.sh), so it must make the same decision as the systemd loops.
ln -s "$PROJECT_ROOT/scripts" "$WORK/scripts"
log "run_long: non-instrumented aflpp driver must warn and label blackbox_n"
cp "$WORK/bin/tool" "$WORK/harnesses/aflpp/onnxruntime_loader_replay"
set +e
LOOP_OUT="$(env -u REQUIRE_NATIVE -u REQUIRE_INSTRUMENTED -u TOOL_AFLPP_CMD \
  WORKDIR="$WORK" DATA_DIR="$WORK/data" TOOL_BIN="$WORK/bin/tool" LOOP_SLEEP_SEC=0 \
  bash "$PROJECT_ROOT/scripts/run_long.sh" --target onnx --backend aflpp \
    --duration-seconds 1 --tag engine-mode-check --corpus-dir "$WORK/seeds/onnx" 2>&1)"
LOOP_EXIT=$?
set -e
[ "$LOOP_EXIT" -eq 0 ] || fail "run_long aflpp exited $LOOP_EXIT: $LOOP_OUT"
assert_contains "WARN"
assert_contains "aflpp_mode=blackbox_n"
assert_contains "AFL_CRASH_EXITCODE=4"
# a corpus that already contains a crashing seed must not abort the whole afl-fuzz run
assert_contains "AFL_IGNORE_SEED_PROBLEMS=1"

log "run_long: REQUIRE_INSTRUMENTED=1 must fail instead of falling back"
set +e
LOOP_OUT="$(env -u TOOL_AFLPP_CMD REQUIRE_INSTRUMENTED=1 \
  WORKDIR="$WORK" DATA_DIR="$WORK/data" TOOL_BIN="$WORK/bin/tool" LOOP_SLEEP_SEC=0 \
  bash "$PROJECT_ROOT/scripts/run_long.sh" --target onnx --backend aflpp \
    --duration-seconds 1 --tag engine-mode-check --corpus-dir "$WORK/seeds/onnx" 2>&1)"
LOOP_EXIT=$?
set -e
[ "$LOOP_EXIT" -ne 0 ] || fail "run_long REQUIRE_INSTRUMENTED=1 must exit non-zero: $LOOP_OUT"

# run_long is the campaign path; it must resolve the gguf driver exactly as the systemd
# loop does, or a campaign and its unit fuzz different things under the same label.
log "run_long: a native gguf driver must label native"
cp "$WORK/bin/tool" "$WORK/harnesses/libfuzzer/gguf_loader_fuzzer"
set +e
LOOP_OUT="$(env -u REQUIRE_NATIVE -u TOOL_LIBFUZZER_CMD \
  WORKDIR="$WORK" DATA_DIR="$WORK/data" TOOL_BIN="$WORK/bin/tool" LOOP_SLEEP_SEC=0 \
  bash "$PROJECT_ROOT/scripts/run_long.sh" --target gguf --backend libfuzzer \
    --duration-seconds 1 --tag engine-mode-check --corpus-dir "$WORK/seeds/gguf" 2>&1)"
LOOP_EXIT=$?
set -e
[ "$LOOP_EXIT" -eq 0 ] || fail "run_long gguf libfuzzer exited $LOOP_EXIT: $LOOP_OUT"
assert_contains "libfuzzer_mode=native"
assert_contains "gguf-native-%p.profraw"

log "run_long: missing native libfuzzer driver must warn and label blackbox"
rm -f "$WORK/harnesses/libfuzzer/onnxruntime_loader_fuzzer"
set +e
LOOP_OUT="$(env -u REQUIRE_NATIVE -u TOOL_LIBFUZZER_CMD \
  WORKDIR="$WORK" DATA_DIR="$WORK/data" TOOL_BIN="$WORK/bin/tool" LOOP_SLEEP_SEC=0 \
  bash "$PROJECT_ROOT/scripts/run_long.sh" --target onnx --backend libfuzzer \
    --duration-seconds 1 --tag engine-mode-check --corpus-dir "$WORK/seeds/onnx" 2>&1)"
LOOP_EXIT=$?
set -e
[ "$LOOP_EXIT" -eq 0 ] || fail "run_long libfuzzer exited $LOOP_EXIT: $LOOP_OUT"
assert_contains "WARN"
assert_contains "libfuzzer_mode=blackbox"

log "done: engine mode labels verified"
