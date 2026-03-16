#!/bin/bash
#
# Contract tests for query implementations (TypeScript, Rust, Go)
#
# These tests verify that all implementations behave identically:
#   1. Exit 1 with usage message when no query arg provided
#   2. Exit 1 with error when invalid JSON variables provided
#   3. Exit 1 with error when no credentials set
#   4. Exit 0 and output valid JSON for a real query (requires LINEAR_API_KEY)
#   5. Exit 1 with GraphQL error for a malformed query
#
# Usage:
#   ./test_contract.sh <binary_path> [label]
#
# Example:
#   ./test_contract.sh "npx tsx ../query.ts" "TypeScript"
#   ./test_contract.sh "./rust/target/release/query" "Rust"
#   ./test_contract.sh "./go/query" "Go"

set -euo pipefail

BINARY="$1"
LABEL="${2:-unknown}"

PASS=0
FAIL=0
SKIP=0

green() { printf "\033[32m%s\033[0m\n" "$1"; }
red()   { printf "\033[31m%s\033[0m\n" "$1"; }
yellow(){ printf "\033[33m%s\033[0m\n" "$1"; }

assert_exit_code() {
  local test_name="$1"
  local expected="$2"
  local actual="$3"

  if [ "$actual" -eq "$expected" ]; then
    green "  PASS: $test_name (exit=$actual)"
    PASS=$((PASS + 1))
  else
    red "  FAIL: $test_name (expected exit=$expected, got exit=$actual)"
    FAIL=$((FAIL + 1))
  fi
}

assert_output_contains() {
  local test_name="$1"
  local pattern="$2"
  local output="$3"

  if echo "$output" | grep -qi "$pattern"; then
    green "  PASS: $test_name (output contains '$pattern')"
    PASS=$((PASS + 1))
  else
    red "  FAIL: $test_name (output missing '$pattern')"
    red "        got: ${output:0:200}"
    FAIL=$((FAIL + 1))
  fi
}

assert_valid_json() {
  local test_name="$1"
  local output="$2"

  if echo "$output" | python3 -m json.tool > /dev/null 2>&1; then
    green "  PASS: $test_name (valid JSON)"
    PASS=$((PASS + 1))
  else
    red "  FAIL: $test_name (invalid JSON)"
    red "        got: ${output:0:200}"
    FAIL=$((FAIL + 1))
  fi
}

echo ""
echo "============================================"
echo "  Contract Tests: $LABEL"
echo "============================================"
echo ""

# -----------------------------------------------
# Test 1: No arguments → exit 1 + usage message
# -----------------------------------------------
echo "Test 1: No arguments"
OUTPUT=$(env -u LINEAR_API_KEY -u LINEAR_AGENT_TOKEN $BINARY 2>&1) || EXIT_CODE=$?
EXIT_CODE=${EXIT_CODE:-0}
assert_exit_code "exits with code 1" 1 "$EXIT_CODE"
assert_output_contains "shows usage/error" "usage\|error\|required\|query" "$OUTPUT"

# -----------------------------------------------
# Test 2: Invalid JSON variables → exit 1
# -----------------------------------------------
echo "Test 2: Invalid JSON variables"
OUTPUT=$(env -u LINEAR_API_KEY -u LINEAR_AGENT_TOKEN $BINARY "query { viewer { id } }" "not-json" 2>&1) || EXIT_CODE=$?
EXIT_CODE=${EXIT_CODE:-0}
# Note: some impls may fail on missing credentials first. That's OK — exit 1 is what matters.
assert_exit_code "exits with code 1" 1 "$EXIT_CODE"

# -----------------------------------------------
# Test 3: No credentials → exit 1 + credential error
# -----------------------------------------------
echo "Test 3: No credentials"
OUTPUT=$(env -u LINEAR_API_KEY -u LINEAR_AGENT_TOKEN $BINARY "query { viewer { id } }" 2>&1) || EXIT_CODE=$?
EXIT_CODE=${EXIT_CODE:-0}
assert_exit_code "exits with code 1" 1 "$EXIT_CODE"
assert_output_contains "mentions credentials" "LINEAR_API_KEY\|LINEAR_AGENT_TOKEN\|credential\|token\|api.key" "$OUTPUT"

# -----------------------------------------------
# Test 4: Valid query (requires LINEAR_API_KEY)
# -----------------------------------------------
echo "Test 4: Valid query (live API)"
if [ -z "${LINEAR_API_KEY:-}" ] && [ -z "${LINEAR_AGENT_TOKEN:-}" ]; then
  yellow "  SKIP: No LINEAR_API_KEY or LINEAR_AGENT_TOKEN set"
  SKIP=$((SKIP + 1))
  SKIP=$((SKIP + 1))
else
  OUTPUT=$($BINARY "query { viewer { id name } }" 2>/dev/null) || EXIT_CODE=$?
  EXIT_CODE=${EXIT_CODE:-0}
  assert_exit_code "exits with code 0" 0 "$EXIT_CODE"
  assert_valid_json "returns valid JSON" "$OUTPUT"
fi

# -----------------------------------------------
# Test 5: Malformed GraphQL → exit 1
# -----------------------------------------------
echo "Test 5: Malformed GraphQL query"
if [ -z "${LINEAR_API_KEY:-}" ] && [ -z "${LINEAR_AGENT_TOKEN:-}" ]; then
  yellow "  SKIP: No LINEAR_API_KEY or LINEAR_AGENT_TOKEN set"
  SKIP=$((SKIP + 1))
else
  OUTPUT=$($BINARY "query { thisFieldDoesNotExist123 }" 2>&1) || EXIT_CODE=$?
  EXIT_CODE=${EXIT_CODE:-0}
  assert_exit_code "exits with code 1" 1 "$EXIT_CODE"
fi

# -----------------------------------------------
# Test 6: Query with valid variables
# -----------------------------------------------
echo "Test 6: Query with variables"
if [ -z "${LINEAR_API_KEY:-}" ] && [ -z "${LINEAR_AGENT_TOKEN:-}" ]; then
  yellow "  SKIP: No LINEAR_API_KEY or LINEAR_AGENT_TOKEN set"
  SKIP=$((SKIP + 1))
  SKIP=$((SKIP + 1))
else
  OUTPUT=$($BINARY 'query($first: Int) { users(first: $first) { nodes { id name } } }' '{"first": 1}' 2>/dev/null) || EXIT_CODE=$?
  EXIT_CODE=${EXIT_CODE:-0}
  assert_exit_code "exits with code 0" 0 "$EXIT_CODE"
  assert_valid_json "returns valid JSON with variables" "$OUTPUT"
fi

# -----------------------------------------------
# Summary
# -----------------------------------------------
echo ""
echo "--------------------------------------------"
TOTAL=$((PASS + FAIL + SKIP))
echo "  $LABEL: $PASS passed, $FAIL failed, $SKIP skipped / $TOTAL total"
if [ "$FAIL" -gt 0 ]; then
  red "  RESULT: FAIL"
  echo "--------------------------------------------"
  exit 1
else
  green "  RESULT: PASS"
  echo "--------------------------------------------"
  exit 0
fi
