#!/bin/bash
# Standard test runner with ALWAYS-ON timeout protection
#
# Usage:
#   ./scripts/run_tests.sh                          # Run all tests
#   ./scripts/run_tests.sh <test_name>             # Run specific test
#   ./scripts/run_tests.sh -- <args>               # Pass args to cargo test
#   ./scripts/run_tests.sh -p <package>            # Run tests for specific package
#   ./scripts/run_tests.sh test-test262            # Run test262 conformance suite
#   ./scripts/run_tests.sh test-conformance        # Run TypeScript conformance suite
#
# This script MUST be used for all test runs to prevent hanging tests.
# It uses cargo-xtask which applies system timeout (300s default).

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

# xtask is not a workspace member, so we invoke it by manifest path.
XTASK="cargo run --manifest-path xtask/Cargo.toml --"

# Dispatch test-conformance to xtask directly
if [ "$1" = "test-conformance" ]; then
    exec $XTASK test-conformance "$@"
fi

# Pass all arguments to xtask
exec $XTASK test "$@"
