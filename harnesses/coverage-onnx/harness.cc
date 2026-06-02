// V2 ONNX coverage harness.
//
// Loads each model file through Ort::Session creation, exercising onnxruntime's
// model parse / load / graph-build / optimizer code paths under LLVM source
// coverage instrumentation. Unlike the existing black-box libfuzzer driver
// (which shells out to a separate, uninstrumented process), this binary links
// the *instrumented* libonnxruntime.so directly, so coverage flows into
// onnxruntime itself.
//
// It intentionally does NOT run inference: input-tensor synthesis is model-
// specific and would add nondeterminism without materially widening coverage of
// the loader, which is the V2 PoC's focus. Malformed/unsupported models are
// expected and counted, not fatal -- the load path is covered either way.
#include <onnxruntime_cxx_api.h>

#include <cstdio>
#include <fstream>
#include <iterator>
#include <vector>

int main(int argc, char** argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: %s <model.onnx> [<model.onnx> ...]\n", argv[0]);
    return 2;
  }

  Ort::Env env(ORT_LOGGING_LEVEL_FATAL, "onnx_cov_harness");

  int loaded = 0, failed = 0;
  for (int i = 1; i < argc; ++i) {
    std::ifstream f(argv[i], std::ios::binary);
    if (!f) {
      ++failed;
      continue;
    }
    std::vector<char> buf((std::istreambuf_iterator<char>(f)),
                          std::istreambuf_iterator<char>());

    Ort::SessionOptions opts;
    opts.SetGraphOptimizationLevel(ORT_ENABLE_ALL);  // exercise optimizer passes
    try {
      Ort::Session session(env, buf.data(), buf.size(), opts);
      ++loaded;
    } catch (const std::exception&) {
      ++failed;  // malformed/unsupported model: load path still covered
    }
  }

  std::fprintf(stderr, "[onnx_cov_harness] loaded=%d failed=%d total=%d\n",
               loaded, failed, argc - 1);
  return 0;
}
