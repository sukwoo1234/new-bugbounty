#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: fuzz_host_preflight.sh [--target <onnx|gguf|safetensors>] [--backends <local-harness,libfuzzer,aflpp>] [--data-dir <dir>] [--seeds-dir <dir>] [--min-free-gb <n>]
EOF
}

WORKDIR="${WORKDIR:-$PWD}"
TARGET="onnx"
BACKENDS="local-harness,libfuzzer,aflpp"
DATA_DIR="$WORKDIR/data"
SEEDS_DIR="$WORKDIR/seeds"
MIN_FREE_GB="50"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="${2:-}"; shift 2 ;;
    --backends) BACKENDS="${2:-}"; shift 2 ;;
    --data-dir) DATA_DIR="${2:-}"; shift 2 ;;
    --seeds-dir) SEEDS_DIR="${2:-}"; shift 2 ;;
    --min-free-gb) MIN_FREE_GB="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[preflight] unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

case "$TARGET" in
  onnx|gguf|safetensors) ;;
  *) echo "[preflight] unsupported target: $TARGET" >&2; exit 2 ;;
esac
if [[ ! "$MIN_FREE_GB" =~ ^[0-9]+$ ]]; then
  echo "[preflight] min-free-gb must be an integer" >&2
  exit 2
fi

IFS=',' read -r -a RAW_BACKENDS <<< "$BACKENDS"
BACKEND_LIST=()
for raw_backend in "${RAW_BACKENDS[@]}"; do
  backend="${raw_backend//[[:space:]]/}"
  if [[ -n "$backend" ]]; then
    case "$backend" in
      local-harness|libfuzzer|aflpp) BACKEND_LIST+=("$backend") ;;
      *) echo "[preflight] unsupported backend: $backend" >&2; exit 2 ;;
    esac
  fi
done
if [[ "${#BACKEND_LIST[@]}" -eq 0 ]]; then
  echo "[preflight] no backends selected" >&2
  exit 2
fi

failures=0
warnings=0

ok() {
  echo "[ok] $*"
}

warn() {
  warnings=$((warnings + 1))
  echo "[warn] $*"
}

fail() {
  failures=$((failures + 1))
  echo "[fail] $*"
}

need_cmd() {
  local cmd="$1"
  if command -v "$cmd" >/dev/null 2>&1; then
    ok "$cmd: $(command -v "$cmd")"
  else
    fail "$cmd not found"
  fi
}

has_backend() {
  local wanted="$1"
  local backend
  for backend in "${BACKEND_LIST[@]}"; do
    if [[ "$backend" == "$wanted" ]]; then
      return 0
    fi
  done
  return 1
}

seed_ext() {
  case "$TARGET" in
    onnx) printf 'onnx' ;;
    gguf) printf 'gguf' ;;
    safetensors) printf 'safetensors' ;;
  esac
}

free_gb_for_path() {
  local path="$1"
  local probe="$path"
  while [[ ! -e "$probe" && "$probe" != "/" ]]; do
    probe="$(dirname "$probe")"
  done
  df -Pk "$probe" | awk 'NR==2 { printf "%d", $4 / 1024 / 1024 }'
}

echo "[preflight] workdir=$WORKDIR"
echo "[preflight] target=$TARGET backends=${BACKEND_LIST[*]}"

need_cmd bash
need_cmd cargo
need_cmd sha256sum

if [[ -f "$WORKDIR/Cargo.toml" ]]; then
  ok "Cargo.toml found"
else
  fail "Cargo.toml not found; run from repo root"
fi

if [[ -x "$WORKDIR/target/debug/tool" ]]; then
  ok "tool binary: target/debug/tool"
else
  fail "target/debug/tool missing; run: cargo build --offline"
fi

target_seed_dir="$SEEDS_DIR/$TARGET"
if [[ -d "$target_seed_dir" ]]; then
  count="$(find "$target_seed_dir" -maxdepth 1 -type f -name "*.$(seed_ext)" | wc -l | tr -d ' ')"
  if [[ "$count" -gt 0 ]]; then
    ok "seeds: $target_seed_dir ($count files)"
  else
    fail "no .$TARGET seed files in $target_seed_dir"
  fi
else
  fail "seed dir not found: $target_seed_dir"
fi

if [[ -d "$DATA_DIR" || -w "$(dirname "$DATA_DIR")" ]]; then
  ok "data dir writable or creatable: $DATA_DIR"
else
  fail "data dir parent is not writable: $(dirname "$DATA_DIR")"
fi

free_gb="$(free_gb_for_path "$DATA_DIR")"
if [[ "$free_gb" -ge "$MIN_FREE_GB" ]]; then
  ok "free disk: ${free_gb}GB >= ${MIN_FREE_GB}GB"
else
  fail "free disk is low: ${free_gb}GB < ${MIN_FREE_GB}GB"
fi

if has_backend libfuzzer; then
  need_cmd clang++
  if [[ "$TARGET" == "onnx" && -x "$WORKDIR/harnesses/libfuzzer/onnxruntime_loader_fuzzer" ]]; then
    ok "libFuzzer native ONNX driver: harnesses/libfuzzer/onnxruntime_loader_fuzzer"
  elif [[ -x "$WORKDIR/harnesses/libfuzzer/tool_harness_driver" ]]; then
    ok "libFuzzer driver: harnesses/libfuzzer/tool_harness_driver"
  else
    fail "libFuzzer driver missing; run: bash scripts/build_libfuzzer_onnx_native.sh or bash scripts/build_libfuzzer_tool_driver.sh"
  fi
fi

if has_backend aflpp; then
  need_cmd docker
  if command -v docker >/dev/null 2>&1; then
    if docker info >/dev/null 2>&1; then
      ok "docker daemon accessible"
    else
      fail "docker daemon inaccessible or permission denied; try: newgrp docker"
    fi
    if docker image inspect aflplusplus/aflplusplus >/dev/null 2>&1; then
      ok "AFL++ image present: aflplusplus/aflplusplus"
    else
      fail "AFL++ image missing; run: docker pull aflplusplus/aflplusplus"
    fi
  fi
fi

echo "[preflight] summary: failures=$failures warnings=$warnings"
if [[ "$failures" -ne 0 ]]; then
  cat <<'EOF'
[preflight] next after fixing failures:
  bash scripts/fuzz_host_preflight.sh --target onnx
  bash scripts/fuzz_smoke.sh --target onnx
EOF
  exit 1
fi

cat <<'EOF'
[preflight] ready
short smoke commands:
  make smoke
  make smoke-all SECONDS=60
or:
  bash scripts/fuzz_smoke.sh
  bash scripts/fuzz_smoke.sh --all --seconds 60
EOF
