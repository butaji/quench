#!/bin/bash
# Process-isolated test runner — runs each test in a subprocess to survive crashes.
# Usage: TEST262_STAGE=16 ./tools/run-each.sh
#
# This is slower than the in-process digest runner but survives stack overflows.
#
# run-test exit codes: 0=pass, 1=fail (error type mismatch), 2=skip,
# 3=negative wrongly passed (fail), 4=harness/read error (fail), 124=timeout.

STAGE=${TEST262_STAGE:-16}
STAGE_DIR=$(grep -A2 "\"id\": $STAGE," tasks/index.json | grep '"path"' | sed 's/.*"path": "\(.*\)",/\1/')
TEST_DIR="tests/test262/$STAGE_DIR"

if [ ! -d "$TEST_DIR" ]; then
    echo "Stage $STAGE directory not found: $TEST_DIR"
    exit 1
fi

echo "=== Process-isolated run: Stage $STAGE ($TEST_DIR) ==="
echo ""

PASSED=0
FAILED=0
SKIPPED=0
TOTAL=0
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# timeout(1) is GNU coreutils — absent on stock macOS.
run_one() {
    if command -v timeout >/dev/null 2>&1; then
        timeout 15 cargo run --bin run-test -- "$1" > "$TMPDIR/out.txt" 2>&1
    else
        cargo run --bin run-test -- "$1" > "$TMPDIR/out.txt" 2>&1
    fi
}

show_failure() {
    local rel="$1" label="$2"
    echo ""
    echo "  $label: $rel"
    if [ $FAILED -le 5 ]; then
        head -3 "$TMPDIR/out.txt" | while read -r line; do echo "    $line"; done
        echo ""
    fi
}

# Process substitution keeps the loop in the current shell so counters propagate.
while read -r test; do
    TOTAL=$((TOTAL + 1))
    REL="${test#$TEST_DIR/}"

    run_one "$test"
    EXIT_CODE=$?

    case $EXIT_CODE in
        0)
            PASSED=$((PASSED + 1))
            printf "\r  Passed: %d  Failed: %d  Skipped: %d  Total: %d" $PASSED $FAILED $SKIPPED $TOTAL
            ;;
        2)
            SKIPPED=$((SKIPPED + 1))
            printf "\r  Passed: %d  Failed: %d  Skipped: %d  Total: %d" $PASSED $FAILED $SKIPPED $TOTAL
            ;;
        124)
            FAILED=$((FAILED + 1))
            show_failure "$REL" "TIMEOUT"
            ;;
        1)
            FAILED=$((FAILED + 1))
            show_failure "$REL" "FAIL"
            ;;
        3)
            FAILED=$((FAILED + 1))
            show_failure "$REL" "FAIL(negative-wrongly-passed)"
            ;;
        4)
            FAILED=$((FAILED + 1))
            show_failure "$REL" "FAIL(harness/read-error)"
            ;;
        *)
            FAILED=$((FAILED + 1))
            show_failure "$REL" "FAIL(exit=$EXIT_CODE)"
            ;;
    esac
done < <(find "$TEST_DIR" -name "*.js" ! -name "*_FIXTURE.js" | sort)

echo ""
echo ""
echo "=== Results: $PASSED passed, $FAILED failed, $SKIPPED skipped ($TOTAL total) ==="
