#!/bin/bash
# Run digest on ALL stages and produce a comprehensive report.
# Usage: bash tools/digest-all.sh
#
# This runs TEST262_DIGEST=1 on every stage and aggregates results.
# Stages that pass 100% are automatically noted.
# Stages that crash (stack overflow) are reported separately.

set -e
cd "$(dirname "$0")/.."

REPORT="tasks/digest-report.md"
echo "# Digest Report — $(date)" > "$REPORT"
echo "" >> "$REPORT"
echo "| Stage | Path | Passed | Total | % |" >> "$REPORT"
echo "|-------|------|--------|-------|---|" >> "$REPORT"

TOTAL_PASSED=0
TOTAL_TESTS=0
CRASHED=""
TIMED_OUT=""
HAS_TIMEOUT=1
command -v timeout >/dev/null 2>&1 || HAS_TIMEOUT=0

while IFS= read -r stage; do
    echo "Stage $stage..."
    STAGE_PATH=$(bash tools/stage-path.sh "$stage")
    STAGE_COUNT=$(bash tools/stage-count.sh "$stage")

    if [ "$STAGE_COUNT" = "0" ] || [ -z "$STAGE_PATH" ]; then
        echo "| $stage | MISSING | - | - | - |" >> "$REPORT"
        continue
    fi

    # Per-stage timeout scaled to its test count: 10s + 0.2s/test, min 120s.
    STAGE_TIMEOUT=$((10 + STAGE_COUNT / 5))
    [ "$STAGE_TIMEOUT" -lt 120 ] && STAGE_TIMEOUT=120

    EXIT_CODE=0
    if [ "$HAS_TIMEOUT" = "1" ]; then
        OUTPUT=$(TEST262_STAGE=$stage TEST262_DIGEST=1 timeout "$STAGE_TIMEOUT" cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1) || EXIT_CODE=$?
    else
        OUTPUT=$(TEST262_STAGE=$stage TEST262_DIGEST=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1) || EXIT_CODE=$?
    fi

    # Digest prints "Passed:  N" and "Total:   N (files)" on separate lines.
    PASSED=$(echo "$OUTPUT" | grep "^Passed:" | head -1 | awk '{print $2}')
    TOTAL=$(echo "$OUTPUT" | grep "^Total:" | head -1 | awk '{print $2}')

    if [ "$EXIT_CODE" = "124" ]; then
        TIMED_OUT="$TIMED_OUT $stage"
        PCT="TIMEOUT"
    elif [ -z "$PASSED" ] || [ -z "$TOTAL" ]; then
        # Stage likely crashed (no digest summary printed)
        CRASHED="$CRASHED $stage"
        PCT="CRASH"
    else
        PCT=$(( (PASSED * 100 + TOTAL / 2) / TOTAL ))
        TOTAL_PASSED=$((TOTAL_PASSED + PASSED))
        TOTAL_TESTS=$((TOTAL_TESTS + TOTAL))
    fi

    echo "| $stage | $STAGE_PATH | ${PASSED:--} | ${TOTAL:--} | $PCT |" >> "$REPORT"
done < <(bash tools/stage-ids.sh)

echo "" >> "$REPORT"
echo "## Summary" >> "$REPORT"
echo "" >> "$REPORT"
echo "**Total**: $TOTAL_PASSED / $TOTAL_TESTS passed" >> "$REPORT"
if [ -n "$CRASHED" ]; then
    echo "**Crashed stages**:$CRASHED" >> "$REPORT"
fi
if [ -n "$TIMED_OUT" ]; then
    echo "**Timed-out stages** (raise timeout or investigate):$TIMED_OUT" >> "$REPORT"
fi
echo "" >> "$REPORT"
echo "Report saved to $REPORT"
cat "$REPORT"
