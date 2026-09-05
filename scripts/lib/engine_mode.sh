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
    if command -v nm >/dev/null 2>&1; then
        # Captured and matched as a here-string, never piped: `nm | grep -q` makes grep
        # close the pipe on its first match, nm dies of SIGPIPE, and all three callers
        # run under `set -o pipefail`, which reads that 141 as "no instrumentation".
        # The symbols matched here (__afl_prev_loc, __afl_shm, __afl_fuzz) are NOT in
        # the raw-bytes list below, so the fallback cannot cover the loss - the arm
        # would just drop to blackbox_n. Reproduced on a 140 KB symbol listing.
        local afl_symbols
        afl_symbols="$(nm -C "${bin}" 2>/dev/null || true)"
        if grep -qE '__afl_(area|prev_loc|shm|fuzz)' <<<"${afl_symbols}"; then
            return 0
        fi
    fi
    grep -qaE '__AFL_SHM_ID|__AFL_SHM_FUZZ_ID|__afl_area_initial|__afl_area_ptr' "${bin}" 2>/dev/null
}

# Which layer AFL++ actually instrumented. has_afl_instrumentation() answers "does this
# binary carry the forkserver", which is a different question: a driver that is
# instrumented but reaches its parser through a separate, uninstrumented .so gives
# driver-level coverage only. The ONNX arm was labelled "instrumented" on exactly that
# basis, and the paper had to say "driver-level" after the fact.
#
#   library      instrumented AND the parser is defined inside this binary
#   driver_only  instrumented, but the parser is not in here - or we cannot tell
#   none         no AFL++ instrumentation at all
#
# Undecidable means driver_only. Claiming a coverage scope we did not verify is the
# error that costs a result, and a stripped binary is undecidable by construction.
#
# LIMIT, on purpose: "the parser is in this binary" is a proxy for "the parser was
# instrumented". A binary that statically links an UNinstrumented archive answers
# `library` here. That gap is closed where it is actually checkable - in
# scripts/build_aflpp_gguf_native.sh, which verifies the archive itself carries the
# AFL++ and ASan symbols before it is ever linked. This function only ever sees the
# finished binary, where the two facts are no longer separable.
#
# has_afl_instrumentation() is deliberately left untouched: three callers depend on it.
TOOL_PARSER_SYMBOLS="${TOOL_PARSER_SYMBOLS:-gguf_init_from_file_impl|onnxruntime::|OrtGetApiBase|safetensors::}"

instrumentation_scope() {
    local bin="$1"

    if ! has_afl_instrumentation "${bin}"; then
        echo "none"
        return 0
    fi
    command -v nm >/dev/null 2>&1 || { echo "driver_only"; return 0; }

    # --defined-only is the whole point: a dynamically linked driver still carries the
    # parser's name in .dynsym as an UNDEFINED reference, so a plain symbol grep - or
    # the raw-bytes fallback has_afl_instrumentation uses - would call every ONNX
    # driver "library".
    #
    # The listing is captured and fed as a here-string, never through a pipe.
    # `nm ... | grep -q` closes the pipe on the first match, nm dies of SIGPIPE, and
    # every caller of this file runs under `set -o pipefail`, which reads that 141 as
    # "symbol not found". Measured on a 398 KB symbol listing, not theorised: the pipe
    # form returns 141 where the here-string form returns 0, and it turned the gguf
    # replay's library scope into driver_only.
    # -C demangles: without it the "onnxruntime::" alternative below can never match,
    # because a C++ symbol reaches us as _ZN11onnxruntime... . gguf_init_from_file_impl
    # matches either way (its mangled form contains the readable name), which is why
    # the gap went unnoticed.
    local symbols
    symbols="$(nm -C --defined-only "${bin}" 2>/dev/null || true)"
    if grep -qE "${TOOL_PARSER_SYMBOLS}" <<<"${symbols}"; then
        echo "library"
        return 0
    fi
    echo "driver_only"
}
