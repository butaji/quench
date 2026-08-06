#!/bin/bash
# Regression test harness — run before every commit/merge
set -e
cd "$(dirname "$0")/.."

echo "=== 1. Build check ==="
cargo check -p quench-runtime 2>&1

echo ""
echo "=== 2. Lint (clippy) ==="
cargo clippy -p quench-runtime --all-targets 2>&1

echo ""
echo "=== 3. Lib tests ==="
cargo nextest run -p quench-runtime --lib 2>&1

echo ""
echo "=== 4. Bootstrap regression tests ==="
cargo nextest run -p quench-runtime --lib -E 'test(bootstrap::tests::*)' 2>&1

echo ""
echo "=== 5. Stage 29 (labeled — should be 23/24) ==="
TEST262_STAGE=29 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1 | tail -3

echo ""
echo "=== 6. Stage 47 (rest-params — should be 11/11) ==="
TEST262_STAGE=47 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1 | tail -3

echo ""
echo "=== 7. Stage 56 (asi — should be 102/102) ==="
TEST262_STAGE=56 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1 | tail -3

echo ""
echo "=== PASS (if no failures above) ==="
