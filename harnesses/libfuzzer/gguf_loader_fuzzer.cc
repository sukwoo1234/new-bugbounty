// Native GGUF loader fuzz harness.
//
// Mirrors harnesses/libfuzzer/onnxruntime_loader_fuzzer.cc: one translation unit
// exporting LLVMFuzzerTestOneInput, plus a GGUF_FUZZ_STANDALONE main() that
// replays files named on argv.
//
// Oracle contract (the whole point of this harness):
//   * a file the parser legitimately REJECTS  -> gguf_init_from_file returns NULL
//     -> we report exit 9. NOT a crash.
//   * a file that trips a GGML_ASSERT / memory error inside ggml -> the process
//     really aborts. That is a REAL finding.
// The harness itself must never be the thing that segfaults, so every ggml
// accessor below is called only under the precondition its own GGML_ASSERTs
// require (correct type dispatch), and every array index is bounded by us
// because gguf_get_arr_str() does NOT bound `i`.

#include "ggml.h"
#include "gguf.h"

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>

#ifndef GGUF_FUZZ_TARGET_ID
#define GGUF_FUZZ_TARGET_ID "unknown"
#endif
#ifndef GGUF_FUZZ_CLAMP_PATCH
#define GGUF_FUZZ_CLAMP_PATCH 0
#endif

#if defined(__has_feature)
#  if __has_feature(address_sanitizer)
#    define GGUF_FUZZ_ASAN 1
#  endif
#endif
#if !defined(GGUF_FUZZ_ASAN) && defined(__SANITIZE_ADDRESS__)
#  define GGUF_FUZZ_ASAN 1
#endif
#ifndef GGUF_FUZZ_ASAN
#  define GGUF_FUZZ_ASAN 0
#endif

// ---------------------------------------------------------------------------
// sanitizer / ggml runtime options
// ---------------------------------------------------------------------------

// Make ASan turn every report into a real abort() so the fuzzer's crash oracle
// and the tool's crash oracle agree. Plain ASan exits with status 1, which
// crash_status_detail() (src/target.rs) does not count as a crash at all - a
// real finding would disappear silently. Also keep leaks out of the signal:
// ggml deliberately does not free some blobs on its error paths.
extern "C" const char *__asan_default_options(void) {
    return "abort_on_error=1:disable_coredump=1:detect_leaks=0";
}

namespace {

// ggml's abort handler walks the stack with backtrace_symbols()/gdb unless this
// is set. That costs ~100 ms per abort and can itself deadlock under a fuzzer,
// so set it ourselves rather than relying on the caller's environment.
struct GgmlEnvInit {
    GgmlEnvInit() { setenv("GGML_NO_BACKTRACE", "1", 1); }
};
GgmlEnvInit g_ggml_env_init;

// What the harness reports back to the tool. Same vocabulary as the
// EXIT_HARNESS_* constants in src/main.rs:45-47.
enum Verdict {
    VERDICT_OK          = 0,   // the parser accepted the file
    VERDICT_REJECTED    = 9,   // the parser turned it down; the input never reached deeper code
    VERDICT_UNAVAILABLE = 10,  // the harness itself cannot run; says nothing about the input
};

// __asan_default_options() is only a default. An ASAN_OPTIONS in the
// environment overrides it, and abort_on_error=0 there would turn every real
// memory finding into a quiet exit(1). Refuse to run rather than fuzz blind.
bool asan_abort_disabled_by_env(void) {
    const char *opts = getenv("ASAN_OPTIONS");
    return opts != nullptr && strstr(opts, "abort_on_error=0") != nullptr;
}

// ---------------------------------------------------------------------------
// depth selection
// ---------------------------------------------------------------------------

enum Depth {
    DEPTH_METADATA = 0,  // ctx = NULL              : header + KV only
    DEPTH_TENSOR_INFO,   // ctx set, no_alloc=true  : + tensor info, no data read
    DEPTH_FULL,          // ctx set, no_alloc=false : + tensor data blob read
};

Depth depth_from_env(void) {
    const char *value = getenv("GGUF_FUZZ_DEPTH");
    if (value == nullptr || *value == '\0') return DEPTH_TENSOR_INFO;
    if (strcmp(value, "metadata") == 0 || strcmp(value, "0") == 0) return DEPTH_METADATA;
    if (strcmp(value, "tensor-info") == 0 || strcmp(value, "1") == 0) return DEPTH_TENSOR_INFO;
    if (strcmp(value, "full") == 0 || strcmp(value, "2") == 0) return DEPTH_FULL;
    return DEPTH_TENSOR_INFO;
}

const char *depth_name(Depth depth) {
    switch (depth) {
        case DEPTH_METADATA:    return "metadata";
        case DEPTH_TENSOR_INFO: return "tensor-info";
        case DEPTH_FULL:        return "full";
    }
    return "tensor-info";
}

// Upper bound on how many elements of one array we touch. Arrays are already
// materialized by the parser, so this only caps wall time on huge vocabs; it is
// not a safety bound (the safety bound is `ne` itself, applied below).
constexpr size_t kMaxArrayElements = 1u << 24;

volatile uint64_t g_sink = 0;

void sink_bytes(const void *p, size_t n) {
    if (p == nullptr || n == 0) return;
    const uint8_t *b = static_cast<const uint8_t *>(p);
    uint64_t acc = 0;
    acc += b[0];
    acc += b[n - 1];
    g_sink += acc;
}

// ---------------------------------------------------------------------------
// KV walk with correct type dispatch
// ---------------------------------------------------------------------------

// Non-string array payload. gguf_get_arr_data() asserts the element type is not
// STRING, so the caller must have dispatched on gguf_get_arr_type() first.
void walk_array_data(const gguf_context *ctx, int64_t key_id, gguf_type arr_type, size_t ne) {
    const void *data = gguf_get_arr_data(ctx, key_id);
    if (data == nullptr || ne == 0) return;

    size_t elem_size = 0;
    switch (arr_type) {
        case GGUF_TYPE_UINT8:
        case GGUF_TYPE_INT8:
        case GGUF_TYPE_BOOL:     elem_size = 1; break;  // bool arrays are int8 on the wire
        case GGUF_TYPE_UINT16:
        case GGUF_TYPE_INT16:    elem_size = 2; break;
        case GGUF_TYPE_UINT32:
        case GGUF_TYPE_INT32:
        case GGUF_TYPE_FLOAT32:  elem_size = 4; break;
        case GGUF_TYPE_UINT64:
        case GGUF_TYPE_INT64:
        case GGUF_TYPE_FLOAT64:  elem_size = 8; break;
        default:                 return;  // STRING handled by caller; ARRAY-of-ARRAY is rejected by the parser
    }
    sink_bytes(data, ne * elem_size);
}

void walk_kv(const gguf_context *ctx) {
    const int64_t n_kv = gguf_get_n_kv(ctx);
    for (int64_t i = 0; i < n_kv; ++i) {
        const char *key = gguf_get_key(ctx, i);
        if (key != nullptr) g_sink += static_cast<uint64_t>(key[0]);

        const gguf_type type = gguf_get_kv_type(ctx, i);

        if (type == GGUF_TYPE_ARRAY) {
            // gguf_get_arr_type() asserts is_array, so it is only legal here.
            const gguf_type arr_type = gguf_get_arr_type(ctx, i);
            const size_t    ne       = gguf_get_arr_n(ctx, i);
            const size_t    limit    = ne < kMaxArrayElements ? ne : kMaxArrayElements;

            if (arr_type == GGUF_TYPE_STRING) {
                // *** INVARIANT: gguf_get_arr_str() does NOT bound `i`
                // (gguf.cpp:820-824) -- it indexes data_string[i] straight
                // through. The bound is ours to apply. (Source-level argument
                // only: ASan does not catch the overrun in practice because the
                // vector's spare capacity absorbs it.)
                for (size_t j = 0; j < limit; ++j) {
                    const char *s = gguf_get_arr_str(ctx, i, j);
                    if (s != nullptr) g_sink += static_cast<uint64_t>(s[0]);
                }
            } else {
                walk_array_data(ctx, i, arr_type, limit);
            }
            continue;
        }

        // *** INVARIANT: every gguf_get_val_*() GGML_ASSERTs both get_ne()==1
        // and an exact type match, so dispatch on the reported type and never
        // guess. Calling a fixed getter would abort the harness itself, which
        // would show up as a brand new fake crash.
        switch (type) {
            case GGUF_TYPE_UINT8:   g_sink += gguf_get_val_u8  (ctx, i); break;
            case GGUF_TYPE_INT8:    g_sink += static_cast<uint64_t>(gguf_get_val_i8 (ctx, i)); break;
            case GGUF_TYPE_UINT16:  g_sink += gguf_get_val_u16 (ctx, i); break;
            case GGUF_TYPE_INT16:   g_sink += static_cast<uint64_t>(gguf_get_val_i16(ctx, i)); break;
            case GGUF_TYPE_UINT32:  g_sink += gguf_get_val_u32 (ctx, i); break;
            case GGUF_TYPE_INT32:   g_sink += static_cast<uint64_t>(gguf_get_val_i32(ctx, i)); break;
            case GGUF_TYPE_FLOAT32: g_sink += static_cast<uint64_t>(gguf_get_val_f32(ctx, i)); break;
            case GGUF_TYPE_BOOL:    g_sink += gguf_get_val_bool(ctx, i) ? 1u : 0u; break;
            case GGUF_TYPE_UINT64:  g_sink += gguf_get_val_u64 (ctx, i); break;
            case GGUF_TYPE_INT64:   g_sink += static_cast<uint64_t>(gguf_get_val_i64(ctx, i)); break;
            case GGUF_TYPE_FLOAT64: g_sink += static_cast<uint64_t>(gguf_get_val_f64(ctx, i)); break;
            case GGUF_TYPE_STRING: {
                const char *s = gguf_get_val_str(ctx, i);
                if (s != nullptr) g_sink += static_cast<uint64_t>(s[0]);
                break;
            }
            default:
                break;  // unreachable: the parser rejects unknown types
        }
    }
}

// ---------------------------------------------------------------------------
// tensor-info walk
// ---------------------------------------------------------------------------

void walk_tensor_info(const gguf_context *ctx, ggml_context *data_ctx, bool touch_data) {
    const int64_t n_tensors = gguf_get_n_tensors(ctx);
    for (int64_t i = 0; i < n_tensors; ++i) {
        const char *name = gguf_get_tensor_name(ctx, i);
        g_sink += static_cast<uint64_t>(gguf_get_tensor_type(ctx, i));
        g_sink += gguf_get_tensor_size(ctx, i);
        g_sink += gguf_get_tensor_offset(ctx, i);
        if (name == nullptr) continue;
        g_sink += static_cast<uint64_t>(name[0]);

        // Cross-check the gguf view against the ggml view. gguf_find_tensor
        // returns -1 rather than aborting when the name is absent.
        g_sink += static_cast<uint64_t>(gguf_find_tensor(ctx, name) + 1);

        if (data_ctx == nullptr) continue;
        ggml_tensor *t = ggml_get_tensor(data_ctx, name);
        if (t == nullptr) continue;
        g_sink += static_cast<uint64_t>(ggml_n_dims(t));
        const size_t nbytes = ggml_nbytes(t);
        g_sink += nbytes;
        if (touch_data && t->data != nullptr) sink_bytes(t->data, nbytes);
    }
}

// ---------------------------------------------------------------------------
// one input
// ---------------------------------------------------------------------------

Verdict run_one_path(const char *path, Depth depth) {
    ggml_context *data_ctx = nullptr;

    gguf_init_params params;
    params.no_alloc = (depth != DEPTH_FULL);
    params.ctx      = (depth == DEPTH_METADATA) ? nullptr : &data_ctx;

    gguf_context *ctx = gguf_init_from_file(path, params);

    // *** THE ORACLE. A NULL return means the parser rejected the file on
    // purpose. That is correct behaviour, not a crash. Dereferencing here is
    // exactly the bug that makes llama-gguf-hash SIGSEGV on a rejected file
    // (gguf-hash.cpp:329-330).
    if (ctx == nullptr) {
        // On the failure path gguf_init_from_file also resets *params.ctx to
        // NULL after freeing it, so there is nothing to release here.
        return VERDICT_REJECTED;
    }

    g_sink += gguf_get_version(ctx);
    g_sink += gguf_get_alignment(ctx);
    g_sink += gguf_get_data_offset(ctx);

    walk_kv(ctx);

    if (depth != DEPTH_METADATA) {
        walk_tensor_info(ctx, data_ctx, depth == DEPTH_FULL);
    }

    gguf_free(ctx);
    if (data_ctx != nullptr) ggml_free(data_ctx);
    return VERDICT_OK;
}

// gguf_init_from_file() takes a path, not a buffer (gguf_init_from_buffer is
// still commented out upstream), so the fuzzer entry point stages the input in
// an anonymous memfd and hands ggml /proc/self/fd/N. No disk I/O, no temp-file
// races between parallel fuzzer workers.
void run_one_buffer(const unsigned char *data, size_t size, Depth depth) {
    const int fd = memfd_create("gguf_fuzz", MFD_CLOEXEC);
    // Swallowing a staging failure would report "nothing wrong here" for an
    // input the parser never saw - a fake clean run, the mirror image of the
    // fake crashes this harness exists to remove. Die loudly instead.
    if (fd < 0) {
        fprintf(stderr, "gguf-harness: memfd_create failed: %s\n", strerror(errno));
        abort();
    }

    size_t written = 0;
    while (written < size) {
        const ssize_t n = write(fd, data + written, size - written);
        if (n <= 0) {
            fprintf(stderr, "gguf-harness: short write staging %zu bytes\n", size);
            abort();
        }
        written += static_cast<size_t>(n);
    }

    char path[64];
    snprintf(path, sizeof(path), "/proc/self/fd/%d", fd);
    run_one_path(path, depth);

    close(fd);
}

}  // namespace

extern "C" int LLVMFuzzerTestOneInput(const unsigned char *data, size_t size) {
    if (data == nullptr || size == 0) return 0;
    run_one_buffer(data, size, depth_from_env());
    return 0;
}

#ifdef GGUF_FUZZ_STANDALONE
namespace {

void print_selftest(void) {
    printf("gguf-harness: selftest ok\n");
    printf("gguf-harness: target=%s\n", GGUF_FUZZ_TARGET_ID);
    printf("gguf-harness: asan=%s\n", GGUF_FUZZ_ASAN ? "on" : "off");
    printf("gguf-harness: asan_default_options=%s\n", __asan_default_options());
    printf("gguf-harness: clamp_patch=%s\n", GGUF_FUZZ_CLAMP_PATCH ? "applied" : "absent");
    printf("gguf-harness: depth=%s\n", depth_name(depth_from_env()));
    printf("gguf-harness: exit_codes ok=%d rejected=%d unavailable=%d\n",
           VERDICT_OK, VERDICT_REJECTED, VERDICT_UNAVAILABLE);
}

}  // namespace

int main(int argc, char **argv) {
    if (asan_abort_disabled_by_env()) {
        fprintf(stderr,
                "gguf-harness: ASAN_OPTIONS sets abort_on_error=0; findings would be "
                "discarded silently. Refusing to run.\n");
        return VERDICT_UNAVAILABLE;
    }

    if (argc >= 2 && strcmp(argv[1], "--selftest") == 0) {
        print_selftest();
        return VERDICT_OK;
    }

    if (argc < 2) {
        fprintf(stderr, "usage: %s [--selftest] <file>...\n", argv[0]);
        return VERDICT_REJECTED;
    }

    const Depth depth = depth_from_env();
    int worst = VERDICT_OK;
    for (int i = 1; i < argc; ++i) {
        // The replay path hands ggml the real file on disk rather than a memfd
        // copy, so a reproduction claim in a report names the same bytes at the
        // same path the operator can open themselves.
        const Verdict v = run_one_path(argv[i], depth);
        if (v != VERDICT_OK) worst = v;
    }
    return worst;
}
#endif
