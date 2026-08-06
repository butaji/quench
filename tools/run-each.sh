#!/usr/bin/env bash
# Process-isolated test runner — runs each test in a subprocess to survive crashes.
# Usage:
#   bash tools/run-each.sh                  # uses TEST262_STAGE if set, else current_stage
#   bash tools/run-each.sh --stage 16
#
# This is slower than the in-process digest runner but survives stack overflows.
#
# run-test exit codes: 0=pass, 1=fail (error type mismatch), 2=skip,
# 3=negative wrongly passed (fail), 4=harness/read error (fail), 124=timeout.

set -euo pipefail
cd "$(dirname "$0")/.."

STAGE="$(bash tools/current-stage.sh)"
if [[ ${#} -gt 0 ]]; then
    case "${1:-}" in
        --stage)
            STAGE="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '1,120p' "$0"
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            exit 1
            ;;
    esac
fi

if [[ "$#" -gt 0 ]]; then
    echo "error: unexpected trailing arguments: $*" >&2
    exit 1
fi

STAGE_DIR="$(bash tools/stage-path.sh "$STAGE")"
TEST_DIR="tests/test262/$STAGE_DIR"
if [[ -z "$STAGE_DIR" || ! -d "$TEST_DIR" ]]; then
    echo "error: stage $STAGE directory not found: $TEST_DIR" >&2
    exit 1
fi

echo "=== Process-isolated run: Stage $STAGE ($TEST_DIR) ==="
echo ""

PASSED=0
FAILED=0
SKIPPED=0
TOTAL=0
LAST_RUN_OUTPUT=""
RUN_TEST_BIN="target/debug/run-test"

if [[ ! -x "$RUN_TEST_BIN" ]]; then
    cargo build -q --bin run-test
fi

# timeout(1) is GNU coreutils — absent on stock macOS.
run_one() {
    set +e
    if command -v timeout >/dev/null 2>&1; then
        LAST_RUN_OUTPUT="$(timeout 15 "$RUN_TEST_BIN" "$1" </dev/null 2>&1)"
    else
        LAST_RUN_OUTPUT="$("$RUN_TEST_BIN" "$1" </dev/null 2>&1)"
    fi
    LAST_RUN_RC=$?
    return "$LAST_RUN_RC"
}

show_failure() {
    local rel="$1" label="$2"
    echo ""
    echo "  $label: $rel"
    if [ $FAILED -le 5 ]; then
        printf '%s\n' "$LAST_RUN_OUTPUT" | head -3 | while read -r line; do echo "    $line"; done
        echo ""
    fi
}

# Process substitution keeps the loop in the current shell so counters propagate.
while read -r test; do
    TOTAL=$((TOTAL + 1))
    REL="${test#$TEST_DIR/}"

    set +e
    run_one "$test"
    EXIT_CODE=$?
    set -e

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

if [[ "$FAILED" -ne 0 || "$SKIPPED" -ne 0 ]]; then
    exit 1
fi
