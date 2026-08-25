# shellcheck shell=bash
# G2/G4: shared engine-mode detection. Sourced by ops/scripts/fuzz-loop-aflpp.sh,
# scripts/run_long.sh and scripts/build_aflpp_onnx_native.sh so the three agree on what
# "instrumented" means. Verified by scripts/check_engine_mode_labels.sh.

# True when the binary carries the AFL++ forkserver runtime, i.e. what afl-fuzz itself
# looks for in check_binary(). Deliberately NOT matching __sanitizer_cov: a plain sancov
# build has no AFL runtime, and driving it without -n makes afl-fuzz abort with
# "No instrumentation detected". __AFL_SHM_ID lives in .rodata, so the grep fallback also
# recognises a stripped instrumented binary, where the symbol names are gone.
has_afl_instrumentation() {
    local bin="$1"

    [ -x "${bin}" ] || return 1
    if command -v nm >/dev/null 2>&1 \
        && nm -C "${bin}" 2>/dev/null | grep -qE '__afl_(area|prev_loc|shm|fuzz)'; then
        return 0
    fi
    grep -qaE '__AFL_SHM_ID|__AFL_SHM_FUZZ_ID|__afl_area_initial|__afl_area_ptr' "${bin}" 2>/dev/null
}
