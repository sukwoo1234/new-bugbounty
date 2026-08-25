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
