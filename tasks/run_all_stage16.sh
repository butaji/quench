#!/bin/bash
# Run all stage 16 tests individually and collect errors
# This avoids the stack overflow from running them all in one process

STAGE_DIR="tests/test262/test/language/statements/class"
ERROR_LOG="/tmp/stage16_errors.txt"
> "$ERROR_LOG"
TOTAL=0
PASSED=0
FAILED=0

echo "Finding tests in $STAGE_DIR..."
TESTS=$(find "$STAGE_DIR" -name "*.js" -path "*/class/*" | sort)
COUNT=$(echo "$TESTS" | wc -l)
echo "Total tests: $COUNT"

echo "Running tests individually..."
for test in $TESTS; do
    TOTAL=$((TOTAL + 1))
    # Use a timeout per test to catch hangs
    result=$(timeout 10 cargo run --bin quench -- "$test" 2>/dev/null || echo "FAIL")
    
    # For now just count - actual running needs the test binary
    if [ "$((TOTAL % 100))" -eq 0 ]; then
        echo "  Progress: $TOTAL / $COUNT"
    fi
done

echo "Done: $TOTAL tests"
