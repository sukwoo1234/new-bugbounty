#!/usr/bin/env bash
set -euo pipefail

# Host bootstrap for fuzzing workers (Ubuntu/WSL).
# Usage:
#   bash scripts/setup_fuzz_host.sh
#   bash scripts/setup_fuzz_host.sh --no-docker

INSTALL_DOCKER=1
if [[ "${1:-}" == "--no-docker" ]]; then
  INSTALL_DOCKER=0
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "[setup] this script currently supports apt-based distros only."
  exit 1
fi

echo "[setup] apt index update"
sudo apt-get update

BASE_PKGS=(
  ca-certificates
  curl
  git
  jq
  tmux
  build-essential
  pkg-config
  python3
  python3-pip
)

echo "[setup] install base packages"
sudo apt-get install -y "${BASE_PKGS[@]}"

echo "[setup] install clang (libfuzzer toolchain prerequisite)"
sudo apt-get install -y clang

# Rust toolchain (cargo) for building this project on fresh hosts.
if ! command -v cargo >/dev/null 2>&1; then
  echo "[setup] install rustup/cargo"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
else
  echo "[setup] cargo already installed"
fi

if [[ "$INSTALL_DOCKER" -eq 1 ]]; then
  echo "[setup] install docker.io (AFL++ docker backend prerequisite)"
  sudo apt-get install -y docker.io
  sudo systemctl enable --now docker >/dev/null 2>&1 || true
  sudo usermod -aG docker "$USER" || true
fi

echo "[setup] verify tools"
echo -n " - clang++: "
clang++ --version | head -n 1 || true
echo -n " - cargo: "
cargo --version || true
echo -n " - docker: "
docker --version || echo "not available (use --no-docker or install Docker Desktop integration)"

cat <<'EOF'
[setup] done
next:
  1) Re-open shell (or run: newgrp docker) if docker group was changed.
  2) Load cargo path in current shell if needed:
     source "$HOME/.cargo/env"
  3) Verify:
     docker run --rm aflplusplus/aflplusplus afl-fuzz -h >/dev/null; echo "EC=$?"
     clang++ --version
     cargo --version
EOF
