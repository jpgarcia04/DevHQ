#!/bin/bash
# Prepares a Claude Code on the web session so that lint, tests, the web build and the Rust
# checks run there as they do on a desktop. Only for remote sessions; idempotent.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

# Tauri 2 on Linux needs WebKitGTK; OpenSSL headers back the native-tls HTTP checks.
if ! pkg-config --exists webkit2gtk-4.1 openssl 2>/dev/null; then
  SUDO=""
  if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
  export DEBIAN_FRONTEND=noninteractive
  $SUDO apt-get update -qq
  $SUDO apt-get install -y -qq --no-install-recommends \
    libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
    libayatana-appindicator3-dev librsvg2-dev build-essential >/dev/null
fi

# npm ci never rewrites the lockfile (an older npm in the container would drop fields).
npm ci --no-audit --no-fund

if command -v cargo >/dev/null; then
  cargo fetch --manifest-path src-tauri/Cargo.toml --locked
fi
