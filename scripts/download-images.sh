#!/bin/bash
#
# Download images from a Linear issue to a local directory.
#
# Uses the Rust binary for authenticated requests with auto token refresh.
# Falls back to TypeScript if Rust is unavailable.
#
# Usage:
#   ./download-images.sh ENG-123
#   ./download-images.sh ENG-123 /tmp/my-images
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_PROJECT_DIR="$SCRIPT_DIR/query-bench/rust"
RUST_BINARY="$RUST_PROJECT_DIR/target/release/download_images"

# --- Load .env ---
if [ -f "$PROJECT_DIR/.env" ]; then
  set -a
  source "$PROJECT_DIR/.env"
  set +a
fi

# --- Credential check ---
if [ -z "${LINEAR_AGENT_TOKEN:-}" ] && [ -z "${LINEAR_API_KEY:-}" ]; then
  echo "Error: LINEAR_AGENT_TOKEN or LINEAR_API_KEY environment variable is required" >&2
  exit 1
fi

# --- Try pre-built Rust binary ---
if [ -x "$RUST_BINARY" ]; then
  exec "$RUST_BINARY" "$@"
fi

# --- Try building with cargo ---
if command -v cargo >/dev/null 2>&1 && [ -f "$RUST_PROJECT_DIR/Cargo.toml" ]; then
  echo "[INFO] Building Rust download_images binary (first run)..." >&2
  if cargo build --release --bin download_images --manifest-path "$RUST_PROJECT_DIR/Cargo.toml" >&2; then
    if [ -x "$RUST_BINARY" ]; then
      exec "$RUST_BINARY" "$@"
    fi
  fi
  echo "[WARN] Rust build failed" >&2
  exit 1
fi

echo "Error: cargo not found and no pre-built binary available" >&2
exit 1
