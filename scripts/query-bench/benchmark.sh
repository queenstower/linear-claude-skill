#!/bin/bash
#
# Benchmark: TypeScript vs Rust vs Go vs Zig for Linear GraphQL queries
#
# Measures:
#   1. Cold-start time (no query, just startup + exit with usage error)
#   2. Live query time (actual API call to Linear)
#
# Usage:
#   LINEAR_API_KEY=lin_api_xxx ./benchmark.sh [iterations]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ITERATIONS="${1:-10}"

echo "============================================"
echo "  Linear Query Benchmark"
echo "  Iterations: $ITERATIONS"
echo "============================================"
echo ""

# Build binaries first
echo "Building binaries..."

# Rust
if [ -f "$SCRIPT_DIR/rust/Cargo.toml" ]; then
  (cd "$SCRIPT_DIR/rust" && PATH="/home/qtame/.cargo/bin:$PATH" cargo build --release 2>&1 | tail -1)
  RUST_BIN="$SCRIPT_DIR/rust/target/release/query"
  echo "  Rust: built"
else
  echo "  Rust: SKIP (no Cargo.toml)"
  RUST_BIN=""
fi

# Go
if [ -f "$SCRIPT_DIR/go/go.mod" ]; then
  (cd "$SCRIPT_DIR/go" && PATH="/usr/local/go/bin:$PATH" go build -o query . 2>&1)
  GO_BIN="$SCRIPT_DIR/go/query"
  echo "  Go: built"
else
  echo "  Go: SKIP (no go.mod)"
  GO_BIN=""
fi

# Zig
if [ -f "$SCRIPT_DIR/zig/build.zig" ] && command -v zig &>/dev/null; then
  if (cd "$SCRIPT_DIR/zig" && zig build -Doptimize=ReleaseFast 2>&1 | tail -1); then
    ZIG_BIN="$SCRIPT_DIR/zig/zig-out/bin/query"
    echo "  Zig: built"
  else
    echo "  Zig: SKIP (build failed)"
    ZIG_BIN=""
  fi
else
  echo "  Zig: SKIP (zig not found or no build.zig)"
  ZIG_BIN=""
fi

# TypeScript - no build needed
TS_CMD="npx tsx $SCRIPT_DIR/../query.ts"
echo "  TypeScript: ready (npx tsx)"
echo ""

# -----------------------------------------------
# Benchmark function
# -----------------------------------------------
bench() {
  local label="$1"
  local cmd="$2"
  local args="$3"
  local iterations="$4"
  local capture_stderr="${5:-false}"

  local total_ms=0
  local min_ms=999999
  local max_ms=0

  for i in $(seq 1 "$iterations"); do
    local start_ns=$(date +%s%N)
    if [ "$capture_stderr" = "true" ]; then
      eval "$cmd $args" > /dev/null 2>&1 || true
    else
      eval "$cmd $args" > /dev/null 2>/dev/null || true
    fi
    local end_ns=$(date +%s%N)
    local elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))
    total_ms=$((total_ms + elapsed_ms))
    [ "$elapsed_ms" -lt "$min_ms" ] && min_ms=$elapsed_ms
    [ "$elapsed_ms" -gt "$max_ms" ] && max_ms=$elapsed_ms
  done

  local avg_ms=$((total_ms / iterations))
  printf "  %-14s avg=%4dms  min=%4dms  max=%4dms  total=%5dms\n" "$label" "$avg_ms" "$min_ms" "$max_ms" "$total_ms"
}

# -----------------------------------------------
# Benchmark 1: Cold start (no args → exit 1)
# -----------------------------------------------
echo "--- Cold Start (startup + exit with error) ---"
echo ""

bench "TypeScript" "$TS_CMD" "" "$ITERATIONS" "true"

if [ -n "$RUST_BIN" ]; then
  bench "Rust" "$RUST_BIN" "" "$ITERATIONS" "true"
fi

if [ -n "$GO_BIN" ]; then
  bench "Go" "$GO_BIN" "" "$ITERATIONS" "true"
fi

if [ -n "${ZIG_BIN:-}" ]; then
  bench "Zig" "$ZIG_BIN" "" "$ITERATIONS" "true"
fi

echo ""

# -----------------------------------------------
# Benchmark 2: Live API query (requires credentials)
# -----------------------------------------------
if [ -n "${LINEAR_API_KEY:-}" ] || [ -n "${LINEAR_AGENT_TOKEN:-}" ]; then
  echo "--- Live API Query: query { viewer { id name } } ---"
  echo ""

  QUERY='"query { viewer { id name } }"'

  bench "TypeScript" "$TS_CMD" "$QUERY" "$ITERATIONS"

  if [ -n "$RUST_BIN" ]; then
    bench "Rust" "$RUST_BIN" "$QUERY" "$ITERATIONS"
  fi

  if [ -n "$GO_BIN" ]; then
    bench "Go" "$GO_BIN" "$QUERY" "$ITERATIONS"
  fi

  if [ -n "${ZIG_BIN:-}" ]; then
    bench "Zig" "$ZIG_BIN" "$QUERY" "$ITERATIONS"
  fi

  echo ""

  # -----------------------------------------------
  # Benchmark 3: Larger query
  # -----------------------------------------------
  echo "--- Live API Query: viewer.assignedIssues (first:10) ---"
  echo ""

  LARGE_QUERY='"query { viewer { assignedIssues(first: 10) { nodes { id identifier title state { name } } } } }"'

  bench "TypeScript" "$TS_CMD" "$LARGE_QUERY" "$ITERATIONS"

  if [ -n "$RUST_BIN" ]; then
    bench "Rust" "$RUST_BIN" "$LARGE_QUERY" "$ITERATIONS"
  fi

  if [ -n "$GO_BIN" ]; then
    bench "Go" "$GO_BIN" "$LARGE_QUERY" "$ITERATIONS"
  fi

  if [ -n "${ZIG_BIN:-}" ]; then
    bench "Zig" "$ZIG_BIN" "$LARGE_QUERY" "$ITERATIONS"
  fi
else
  echo "--- Live API Query: SKIPPED (no LINEAR_API_KEY or LINEAR_AGENT_TOKEN) ---"
fi

echo ""
echo "============================================"
echo "  Benchmark complete"
echo "============================================"
