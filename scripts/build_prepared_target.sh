#!/usr/bin/env bash
set -euo pipefail

DATA_DIR="${1:-}"
TARGET="${2:-}"
VERSION="${3:-}"

if [[ -z "$DATA_DIR" || -z "$TARGET" ]]; then
  echo "[target-build] usage: build_prepared_target.sh <data_dir> <target> [version]" >&2
  exit 2
fi

case "$TARGET" in
  gguf)
    TARGET_NAME="llama.cpp"
    ;;
  onnx)
    TARGET_NAME="onnxruntime"
    ;;
  safetensors)
    TARGET_NAME="safetensors"
    ;;
  *)
    echo "[target-build] unsupported target: $TARGET" >&2
    exit 2
    ;;
esac

TARGET_ROOT="$DATA_DIR/targets/$TARGET_NAME"
if [[ ! -d "$TARGET_ROOT" ]]; then
  echo "[target-build] prepared target not found: $TARGET_ROOT" >&2
  exit 3
fi

if [[ -z "$VERSION" ]]; then
  VERSION="$(find "$TARGET_ROOT" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort | tail -n 1)"
fi

if [[ -z "$VERSION" ]]; then
  echo "[target-build] no prepared version found for $TARGET" >&2
  exit 3
fi

VERSION_ROOT="$TARGET_ROOT/$VERSION"
SOURCE_DIR="$VERSION_ROOT/source"
ARCHIVE_PATH="$(find "$SOURCE_DIR" -maxdepth 1 -type f | sort | head -n 1)"

if [[ -z "$ARCHIVE_PATH" || ! -f "$ARCHIVE_PATH" ]]; then
  echo "[target-build] source archive not found in $SOURCE_DIR" >&2
  exit 4
fi

EXTRACT_DIR="$VERSION_ROOT/build-src"
BUILD_DIR="$VERSION_ROOT/build-out"
rm -rf "$EXTRACT_DIR"
mkdir -p "$EXTRACT_DIR"

echo "[target-build] target: $TARGET"
echo "[target-build] version: $VERSION"
echo "[target-build] archive: $ARCHIVE_PATH"

tar -xf "$ARCHIVE_PATH" -C "$EXTRACT_DIR"
SRC_ROOT="$(find "$EXTRACT_DIR" -mindepth 1 -maxdepth 1 -type d | sort | head -n 1)"

if [[ -z "$SRC_ROOT" || ! -d "$SRC_ROOT" ]]; then
  echo "[target-build] extracted source directory not found" >&2
  exit 5
fi

case "$TARGET" in
  gguf)
    cmake -S "$SRC_ROOT" -B "$BUILD_DIR" -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=ON
    cmake --build "$BUILD_DIR" -j2
    ARTIFACT="$BUILD_DIR/bin/llama-cli"
    if [[ ! -f "$ARTIFACT" ]]; then
      echo "[target-build] expected artifact missing: $ARTIFACT" >&2
      exit 6
    fi
    ;;
  onnx|safetensors)
    echo "[target-build] build path not yet implemented for $TARGET" >&2
    exit 7
    ;;
esac

echo "[target-build] done"
echo "build_dir: $BUILD_DIR"
echo "artifact: $ARTIFACT"
