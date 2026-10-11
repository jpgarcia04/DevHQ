#!/bin/bash
# Prepares a Claude Code on the web session so that lint, tests, the web build and the Rust
# checks run there as they do on a desktop. Only for remote sessions; idempotent.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
export DEBIAN_FRONTEND=noninteractive
packages=()
# Tauri 2 on Linux needs WebKitGTK; OpenSSL headers back the native-tls HTTP checks.
if ! pkg-config --exists webkit2gtk-4.1 openssl 2>/dev/null; then
  packages+=(libwebkit2gtk-4.1-dev libxdo-dev libssl-dev
    libayatana-appindicator3-dev librsvg2-dev build-essential)
fi
# The native end-to-end walk (npm run e2e:native) drives the real window through
# WebKitWebDriver on a virtual X display.
if ! command -v WebKitWebDriver >/dev/null || ! command -v Xvfb >/dev/null; then
  packages+=(webkit2gtk-driver xvfb)
fi
if [ ${#packages[@]} -gt 0 ]; then
  $SUDO apt-get update -qq
  $SUDO apt-get install -y -qq --no-install-recommends "${packages[@]}" >/dev/null
fi

# DevHQ tells this computer from another by a fingerprint of /etc/machine-id, and the native walk
# chooses this computer in Settings. A bare container can have none, or an empty one, as systemd
# leaves it in images: give it one of its own, once. A desktop or a CI runner already has one.
if [ ! -s /etc/machine-id ]; then
  tr -d '-' < /proc/sys/kernel/random/uuid | $SUDO tee /etc/machine-id >/dev/null
fi

# The lockfile is written by npm 11, as on desktops and in CI (Node 24). npm 10, which the
# container may bring, drops the libc fields of every platform-specific package whenever it
# rewrites the lockfile, so adding a dependency here would also rewrite unrelated entries.
# Upgrading the container's own npm makes plain `npm` safe; failing to do so must not stop
# the rest of the setup, so it only warns, and the warning reaches the session.
if [ "$(npm --version | cut -d. -f1)" -lt 11 ]; then
  if ! npm install --global --no-audit --no-fund npm@11 >/dev/null 2>&1; then
    echo "WARNING: npm $(npm --version) could not be upgraded to npm 11." \
      "Do not add, remove or update npm dependencies in this session."
  fi
fi

# npm ci never rewrites the lockfile. It reinstalls from scratch, so it runs only when the
# lockfile changed since the last install.
stamp=node_modules/.devhq-lock.sha256
if ! sha256sum --check --status "$stamp" 2>/dev/null; then
  npm ci --no-audit --no-fund
  sha256sum package-lock.json > "$stamp"
fi

# npm run e2e never downloads a browser: point it at the Chromium the session already has.
chromium=$(ls -d /opt/pw-browsers/chromium-*/chrome-linux/chrome 2>/dev/null | sort -V | tail -1 || true)
if [ -n "$chromium" ] && [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  echo "export DEVHQ_CHROMIUM=$chromium" >> "$CLAUDE_ENV_FILE"
fi

if command -v cargo >/dev/null; then
  cargo fetch --manifest-path src-tauri/Cargo.toml --locked
  # tauri-driver sits between the walk and WebKitWebDriver. Same version as in CI.
  if ! cargo install --list | grep -q '^tauri-driver v2.1.0:'; then
    cargo install tauri-driver --version 2.1.0 --locked --quiet
  fi
fi
