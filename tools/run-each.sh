#!/bin/bash
# Process-isolated test runner — runs each test in a subprocess to survive crashes.
# Usage: TEST262_STAGE=16 ./tools/run-each.sh
#
# This is slower than the in-process digest runner but survives stack overflows.

STAGE=${TEST262_STAGE:-16}
STAGE_DIR=$(grep -A2 "\"id\": $STAGE" tasks/index.json | grep '"path"' | sed 's/.*"path": "\(.*\)",/\1/')
TEST_DIR="tests/test262/$STAGE_DIR"

if [ ! -d "$TEST_DIR" ]; then
    echo "Stage $STAGE directory not found: $TEST_DIR"
    exit 1
fi

echo "=== Process-isolated run: Stage $STAGE ($TEST_DIR) ==="
echo ""

PASSED=0
FAILED=0
TOTAL=0
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

find "$TEST_DIR" -name "*.js" ! -name "*_FIXTURE.js" | sort | while read test; do
    TOTAL=$((TOTAL + 1))
    REL="${test#$TEST_DIR/}"
    
    # Run with timeout
    timeout 15 cargo run --bin run-test -- "$test" > "$TMPDIR/out.txt" 2>&1
    EXIT_CODE=$?
    
    if [ $EXIT_CODE -eq 0 ]; then
        PASSED=$((PASSED + 1))
        printf "\r  Passed: %d  Failed: %d  Total: %d" $PASSED $FAILED $TOTAL
    elif [ $EXIT_CODE -eq 124 ]; then
        FAILED=$((FAILED + 1))
        echo ""
        echo "  TIMEOUT: $REL"
    else
        FAILED=$((FAILED + 1))
        # For first few failures, show details
        if [ $FAILED -le 5 ]; then
            echo ""
            echo "  FAIL: $REL"
            head -3 "$TMPDIR/out.txt" | while read line; do echo "    $line"; done
            echo ""
        fi
    fi
done

echo ""
echo ""
echo "=== Results: $PASSED passed, $FAILED failed ==="
