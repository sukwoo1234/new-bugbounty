#!/usr/bin/env bash
set -euo pipefail

# Reproduce the three GGUF parser aborts (V1/V2/V3) on a STOCK, unpatched, ASan-free
# build of the pinned llama.cpp b7921 - the evidence an upstream report rests on.
#
# Unlike the fuzzing harness this links no sanitizer and applies no source patch: it
# calls gguf_init_from_file, the entry point every app that loads a GGUF model uses,
# against libggml-base.a built straight from the official b7921 tarball. So a maintainer
# can see the abort is in the library itself, not an artifact of how we fuzz it.
#
# Prerequisites, both produced by earlier steps and gitignored (under /data, /seeds):
#   - the plain reference library   scripts/build_prepared_target.sh  (build-out-plain)
#   - the PoC seeds                  scripts/gen_gguf_malformed_seeds.sh
#
# Exit 0 only if all three PoCs abort (134) at the expected upstream line and the
# well-formed control loads cleanly.

PROJECT_ROOT="${PROJECT_ROOT:-$(pwd)}"
VERSION="${VERSION:-b7921}"
VROOT="$PROJECT_ROOT/data/targets/llama.cpp/$VERSION"
PLAIN_LIB="${PLAIN_LIB:-$VROOT/build-out-plain/ggml/src/libggml-base.a}"
PLAIN_SRC="${PLAIN_SRC:-$VROOT/build-src-plain/llama.cpp-$VERSION}"
SEED_ROOT="${SEED_ROOT:-$PROJECT_ROOT/seeds}"
# The pinned gguf.cpp, byte-identical to the official tarball. The line numbers below
# are this file's; the fuzz build restores them with #line so its aborts match too.
GGUF_SHA256="1eedaef85118cc8840f5e7bebbbcd03330421793afc0a8d5e2efdf9c7bd5967b"

log() { echo "[repro-gguf] $*"; }
fail() { echo "[repro-gguf] fail: $*" >&2; exit 1; }

[ -f "$PLAIN_LIB" ] || fail "plain library not found: $PLAIN_LIB (build it: scripts/build_prepared_target.sh)"
[ -f "$PLAIN_SRC/ggml/include/gguf.h" ] || fail "plain headers not found under $PLAIN_SRC"

# The library must really be the stock build: no ASan, no clamp. If this ever links the
# fuzz build by mistake, the "reproduces on stock upstream" claim would be false.
if nm "$PLAIN_LIB" 2>/dev/null | grep -q "__asan"; then
  fail "$PLAIN_LIB carries ASan symbols; this is not the plain reference build"
fi
actual_sha="$(sha256sum "$PLAIN_SRC/ggml/src/gguf.cpp" | cut -d' ' -f1)"
[ "$actual_sha" = "$GGUF_SHA256" ] \
  || fail "gguf.cpp sha256 $actual_sha != pinned $GGUF_SHA256 (source is not stock $VERSION)"

# Regenerate the PoC seeds so the run does not depend on a leftover /seeds state.
log "generating PoC seeds"
bash "$PROJECT_ROOT/scripts/gen_gguf_malformed_seeds.sh" >/dev/null

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cat > "$WORK/repro.c" <<'EOF'
#include <stdio.h>
#include "gguf.h"
int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: %s <file.gguf>\n", argv[0]); return 2; }
    struct gguf_init_params p = { /*no_alloc=*/false, /*ctx=*/NULL };
    struct gguf_context *ctx = gguf_init_from_file(argv[1], p);
    if (!ctx) { printf("REJECTED cleanly (returned NULL)\n"); return 0; }
    printf("LOADED ok (n_kv=%ld)\n", (long)gguf_get_n_kv(ctx));
    gguf_free(ctx);
    return 0;
}
EOF

CXX="${CXX:-g++}"
log "compiling standalone reproducer against the stock library"
"$CXX" -O1 -I"$PLAIN_SRC/ggml/include" "$WORK/repro.c" "$PLAIN_LIB" -lm -lpthread -o "$WORK/repro" \
  || fail "reproducer failed to link against $PLAIN_LIB"

# PoC file <TAB> expected abort line <TAB> what is unchecked
CASES=$(cat <<'EOF'
gguf-malformed/align_wrongtype.gguf	183	V1 general.alignment value type is not checked before gguf_get_val_u32
gguf-malformed/align_array2.gguf	864	V2 general.alignment array arity is not checked (get_ne()==1)
gguf-malformed/emptykey.gguf	132	V3 a zero-length KV key is asserted, not rejected
EOF
)

status=0
while IFS=$'\t' read -r rel line what; do
  [ -n "$rel" ] || continue
  path="$SEED_ROOT/$rel"
  [ -f "$path" ] || fail "PoC seed missing: $path"
  set +e
  out="$("$WORK/repro" "$path" 2>&1)"
  rc=$?
  set -e
  if [ "$rc" -ne 134 ]; then
    log "MISS $rel: expected SIGABRT (134), got exit $rc"
    status=1
    continue
  fi
  if ! printf '%s' "$out" | grep -q "gguf.cpp:$line:"; then
    log "MISS $rel: aborted, but not at gguf.cpp:$line"
    printf '%s\n' "$out" | grep -i "gguf.cpp" | head -1
    status=1
    continue
  fi
  log "OK   $rel -> gguf.cpp:$line SIGABRT  ($what)"
done <<< "$CASES"

# The control must load: it is the same shape as the PoCs but well formed, so a clean
# load proves the abort is the malformation and not the harness.
set +e
out="$("$WORK/repro" "$SEED_ROOT/gguf/align_ok.gguf" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -q "LOADED ok"; then
  log "OK   control gguf/align_ok.gguf loads cleanly"
else
  log "MISS control align_ok.gguf did not load (exit $rc): $out"
  status=1
fi

[ "$status" -eq 0 ] || fail "one or more PoCs did not reproduce as expected"
log "all three GGUF asserts reproduced on stock $VERSION (no ASan, no source patch)"
