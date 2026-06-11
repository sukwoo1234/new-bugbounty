#include <onnxruntime_cxx_api.h>

#include <algorithm>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <fstream>
#include <iterator>
#include <limits>
#include <string>
#include <unistd.h>
#include <vector>

namespace {

constexpr size_t kDefaultMaxTensorElements = 100000;
constexpr size_t kMaxInputs = 32;
constexpr size_t kMaxOutputs = 32;

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

bool inference_enabled_from_env() {
  const char* value = std::getenv("ONNX_FUZZ_RUN_INFERENCE");
  return value == nullptr || std::strcmp(value, "0") != 0;
}

size_t max_tensor_elements_from_env() {
  const char* value = std::getenv("ONNX_FUZZ_MAX_TENSOR_ELEMENTS");
  if (value == nullptr || *value == '\0') return kDefaultMaxTensorElements;
  char* end = nullptr;
  unsigned long long parsed = std::strtoull(value, &end, 10);
  if (end == value || parsed == 0) return kDefaultMaxTensorElements;
  return static_cast<size_t>(
      std::min<unsigned long long>(parsed, static_cast<unsigned long long>(kDefaultMaxTensorElements)));
}

size_t element_size(ONNXTensorElementDataType type) {
  switch (type) {
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_BOOL:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT8:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT8:
      return 1;
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT16:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_BFLOAT16:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT16:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT16:
      return 2;
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT32:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT32:
      return 4;
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_DOUBLE:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64:
    case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT64:
      return 8;
    default:
      return 0;
  }
}

std::vector<int64_t> bounded_shape(std::vector<int64_t> shape, size_t max_elements) {
  if (shape.empty()) return shape;
  size_t element_count = 1;
  for (int64_t& dimension : shape) {
    if (dimension <= 0) dimension = 1;
    if (dimension > 1024) dimension = 1024;
    const size_t dim = static_cast<size_t>(dimension);
    if (element_count > max_elements / dim) {
      dimension = 1;
    }
    element_count *= static_cast<size_t>(dimension);
  }
  return shape;
}

size_t shape_element_count(const std::vector<int64_t>& shape) {
  if (shape.empty()) return 1;
  size_t element_count = 1;
  for (int64_t dimension : shape) {
    if (dimension <= 0) return 0;
    const size_t dim = static_cast<size_t>(dimension);
    if (element_count > std::numeric_limits<size_t>::max() / dim) return 0;
    element_count *= dim;
  }
  return element_count;
}

void fill_input_buffer(std::vector<uint8_t>& buffer, const unsigned char* data, size_t size) {
  if (buffer.empty()) return;
  static constexpr uint8_t boundaries[] = {
      0x00, 0x01, 0x7f, 0x80, 0xff, 0x00, 0x00, 0x80, 0x7f, 0xff, 0xff, 0xff,
  };
  for (size_t index = 0; index < buffer.size(); ++index) {
    const uint8_t from_input = size == 0 ? 0 : data[index % size];
    buffer[index] = static_cast<uint8_t>(from_input ^ boundaries[index % sizeof(boundaries)]);
  }
}

bool append_input_tensor(Ort::Session& session, size_t input_index, Ort::AllocatorWithDefaultOptions& allocator,
                         Ort::MemoryInfo& memory_info, const unsigned char* data, size_t size,
                         size_t max_elements, std::vector<std::string>& input_name_storage,
                         std::vector<const char*>& input_names, std::vector<std::vector<uint8_t>>& input_buffers,
                         std::vector<Ort::Value>& input_tensors) {
  auto name = session.GetInputNameAllocated(input_index, allocator);
  input_name_storage.push_back(name.get());
  input_names.push_back(input_name_storage.back().c_str());

  auto type_info = session.GetInputTypeInfo(input_index);
  auto tensor_info = type_info.GetTensorTypeAndShapeInfo();
  const ONNXTensorElementDataType tensor_type = tensor_info.GetElementType();
  const size_t bytes_per_element = element_size(tensor_type);
  if (bytes_per_element == 0) return false;

  std::vector<int64_t> shape = bounded_shape(tensor_info.GetShape(), max_elements);
  const size_t element_count = shape_element_count(shape);
  if (element_count == 0 || element_count > max_elements) return false;
  const size_t byte_count = element_count * bytes_per_element;
  input_buffers.emplace_back(byte_count);
  fill_input_buffer(input_buffers.back(), data, size);
  input_tensors.push_back(Ort::Value::CreateTensor(memory_info, input_buffers.back().data(), byte_count, shape.data(),
                                                   shape.size(), tensor_type));
  return true;
}

void run_dummy_inference(Ort::Session& session, const unsigned char* data, size_t size) {
  const size_t input_count = session.GetInputCount();
  const size_t output_count = session.GetOutputCount();
  if (input_count > kMaxInputs || output_count == 0 || output_count > kMaxOutputs) return;

  Ort::AllocatorWithDefaultOptions allocator;
  Ort::MemoryInfo memory_info = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
  std::vector<std::string> input_name_storage;
  std::vector<std::string> output_name_storage;
  std::vector<const char*> input_names;
  std::vector<const char*> output_names;
  std::vector<std::vector<uint8_t>> input_buffers;
  std::vector<Ort::Value> input_tensors;

  input_name_storage.reserve(input_count);
  output_name_storage.reserve(output_count);
  input_names.reserve(input_count);
  output_names.reserve(output_count);
  input_buffers.reserve(input_count);
  input_tensors.reserve(input_count);

  const size_t max_elements = max_tensor_elements_from_env();
  for (size_t input_index = 0; input_index < input_count; ++input_index) {
    if (!append_input_tensor(session, input_index, allocator, memory_info, data, size, max_elements, input_name_storage,
                             input_names, input_buffers, input_tensors)) {
      return;
    }
  }

  for (size_t output_index = 0; output_index < output_count; ++output_index) {
    auto output_name = session.GetOutputNameAllocated(output_index, allocator);
    output_name_storage.push_back(output_name.get());
    output_names.push_back(output_name_storage.back().c_str());
  }

  Ort::RunOptions run_options;
  std::vector<Ort::Value> outputs = session.Run(run_options, input_names.data(), input_tensors.data(),
                                                input_tensors.size(), output_names.data(), output_names.size());
  volatile size_t observed_outputs = outputs.size();
  (void)observed_outputs;
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
    if (inference_enabled_from_env()) {
      run_dummy_inference(session, data, size);
    }
  } catch (const Ort::Exception&) {
    return 0;
  } catch (const std::exception&) {
    return 0;
  }

  return 0;
}

#ifdef ONNX_FUZZ_STANDALONE
int main(int argc, char** argv) {
  if (argc < 2) return 0;
  for (int arg_index = 1; arg_index < argc; ++arg_index) {
    std::ifstream input(argv[arg_index], std::ios::binary);
    if (!input) continue;
    std::vector<unsigned char> buffer((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
    if (!buffer.empty()) {
      LLVMFuzzerTestOneInput(buffer.data(), buffer.size());
    }
  }
  return 0;
}
#endif
