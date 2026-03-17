#!/bin/bash
#
# Smart dispatcher for Linear GraphQL queries.
#
# Prefers the Rust binary for speed. Falls back to TypeScript (npx tsx).
#
# Resolution order:
#   1. Pre-built Rust binary (scripts/query-bench/rust/target/release/query)
#   2. Build via cargo if available, then run the binary
#   3. Fall back to npx tsx scripts/query.ts
#
# Usage:
#   LINEAR_API_KEY=lin_api_xxx ./query.sh "query { viewer { id name } }"
#   LINEAR_API_KEY=lin_api_xxx ./query.sh "query { viewer { id } }" '{"var": "val"}'
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_PROJECT_DIR="$SCRIPT_DIR/query-bench/rust"
RUST_BINARY="$RUST_PROJECT_DIR/target/release/query"

# --- Load .env (takes precedence over stale shell env vars) ---
if [ -f "$PROJECT_DIR/.env" ]; then
  set -a
  source "$PROJECT_DIR/.env"
  set +a
fi

# --- Credential check (shared by all backends) ---
if [ -z "${LINEAR_AGENT_TOKEN:-}" ] && [ -z "${LINEAR_API_KEY:-}" ]; then
  echo "Error: LINEAR_AGENT_TOKEN or LINEAR_API_KEY environment variable is required" >&2
  echo "" >&2
  echo "Usage:" >&2
  echo "  npx tsx scripts/oauth-setup.ts  # Set up agent identity (preferred)" >&2
  echo "  LINEAR_API_KEY=lin_api_xxx ./query.sh \"query { viewer { id name } }\"" >&2
  exit 1
fi

# --- Try Rust binary ---
if [ -x "$RUST_BINARY" ]; then
  exec "$RUST_BINARY" "$@"
fi

# --- Try building with cargo ---
if command -v cargo >/dev/null 2>&1 && [ -f "$RUST_PROJECT_DIR/Cargo.toml" ]; then
  echo "[INFO] Building Rust query binary (first run)..." >&2
  if cargo build --release --manifest-path "$RUST_PROJECT_DIR/Cargo.toml" >&2; then
    if [ -x "$RUST_BINARY" ]; then
      exec "$RUST_BINARY" "$@"
    fi
  fi
  echo "[WARN] Rust build failed, falling back to TypeScript" >&2
fi

# --- Fallback to TypeScript ---
exec npx tsx "$SCRIPT_DIR/query.ts" "$@"
