#include <onnxruntime_cxx_api.h>

#include <cstdlib>
#include <cstring>
#include <exception>

namespace {

GraphOptimizationLevel graph_opt_level_from_env() {
  const char* value = std::getenv("ONNX_FUZZ_OPT_LEVEL");
  if (value == nullptr) return ORT_ENABLE_ALL;
  if (std::strcmp(value, "DISABLE_ALL") == 0 || std::strcmp(value, "0") == 0) {
    return ORT_DISABLE_ALL;
  }
  if (std::strcmp(value, "BASIC") == 0 || std::strcmp(value, "1") == 0) {
    return ORT_ENABLE_BASIC;
  }
  if (std::strcmp(value, "EXTENDED") == 0 || std::strcmp(value, "2") == 0) {
    return ORT_ENABLE_EXTENDED;
  }
  return ORT_ENABLE_ALL;
}

}  // namespace

extern "C" int LLVMFuzzerTestOneInput(const unsigned char* data, size_t size) {
  static Ort::Env env(ORT_LOGGING_LEVEL_FATAL, "onnxruntime_loader_fuzzer");
  if (data == nullptr || size == 0) return 0;

  Ort::SessionOptions opts;
  opts.SetIntraOpNumThreads(1);
  opts.SetInterOpNumThreads(1);
  opts.DisableMemPattern();
  opts.SetGraphOptimizationLevel(graph_opt_level_from_env());

  try {
    Ort::Session session(env, data, size, opts);
    volatile size_t input_count = session.GetInputCount();
    volatile size_t output_count = session.GetOutputCount();
    (void)input_count;
    (void)output_count;
  } catch (const Ort::Exception&) {
    return 0;
  } catch (const std::exception&) {
    return 0;
  }

  return 0;
}
