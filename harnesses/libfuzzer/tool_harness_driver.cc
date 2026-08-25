#include <atomic>
#include <csignal>
#include <cstdlib>
#include <fstream>
#include <string>
#include <sys/wait.h>
#include <unistd.h>

namespace {

std::atomic<unsigned long long> g_seq{0};

// G3: `tool harness` exit codes (kept in sync with EXIT_HARNESS_* in src/main.rs).
// 4 means the target library crashed - the finding. 9 means the harness rejected the
// input before the library ran (missing input, precheck reject, probe unavailable);
// aborting on that would file every rejected mutant as a libFuzzer crash artifact.
constexpr int kToolHarnessLibraryCrashExit = 4;
constexpr int kToolHarnessBenignExit = 9;

std::string env_or(const char* key, const char* fallback) {
  const char* v = std::getenv(key);
  if (v && *v) return std::string(v);
  return std::string(fallback);
}

std::string shell_escape(const std::string& s) {
  std::string out;
  out.reserve(s.size() + 8);
  out.push_back('\'');
  for (char c : s) {
    if (c == '\'') out += "'\\''";
    else out.push_back(c);
  }
  out.push_back('\'');
  return out;
}

}  // namespace

extern "C" int LLVMFuzzerTestOneInput(const unsigned char* data, size_t size) {
  const std::string tool_bin = env_or("TOOL_HARNESS_TOOL", "./target/debug/tool");
  const std::string target = env_or("TOOL_HARNESS_TARGET", "onnx");
  const std::string ext = env_or("TOOL_HARNESS_EXT", "onnx");

  const unsigned long long seq = g_seq.fetch_add(1, std::memory_order_relaxed);
  const std::string input_path = "/tmp/tool-libfuzz-" + std::to_string(getpid()) + "-" +
                                 std::to_string(seq) + "." + ext;

  {
    std::ofstream ofs(input_path, std::ios::binary | std::ios::trunc);
    if (!ofs) return 0;
    ofs.write(reinterpret_cast<const char*>(data), static_cast<std::streamsize>(size));
  }

  const std::string cmd =
      shell_escape(tool_bin) + " harness --target " + shell_escape(target) +
      " --input " + shell_escape(input_path) + " >/dev/null 2>&1";
  const int rc = std::system(cmd.c_str());
  std::remove(input_path.c_str());
  if (rc == -1) {
    std::abort();
  }
  if (WIFSIGNALED(rc)) {
    std::raise(WTERMSIG(rc));
  }
  if (!WIFEXITED(rc)) {
    std::abort();
  }
  const int status = WEXITSTATUS(rc);
  if (status == kToolHarnessBenignExit) {
    return 0;
  }
  // Everything else that is non-zero - kToolHarnessLibraryCrashExit, a shell-reported
  // 128+signal, an unknown code - stays a finding, so a real crash is never dropped.
  if (status != 0) {
    std::abort();
  }
  return 0;
}
