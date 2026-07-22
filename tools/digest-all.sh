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

for stage in $(seq 0 121); do
    echo "Stage $stage..."
    STAGE_INFO=$(python3 -c "
import json
with open('tasks/index.json') as f:
    d = json.load(f)
for s in d['stages']:
    if s['id'] == $stage:
        print(f\"{s['path']}|{s['tests']}\")
        break
" 2>/dev/null || echo "unknown|0")
    
    STAGE_PATH=$(echo "$STAGE_INFO" | cut -d'|' -f1)
    STAGE_COUNT=$(echo "$STAGE_INFO" | cut -d'|' -f2)
    
    if [ "$STAGE_COUNT" = "0" ] || [ -z "$STAGE_PATH" ]; then
        echo "| $stage | MISSING | - | - | - |" >> "$REPORT"
        continue
    fi
    
    # Run digest with timeout (60s per stage)
    OUTPUT=$(TEST262_STAGE=$stage TEST262_DIGEST=1 timeout 60 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture 2>&1 || true)
    
    PASSED=$(echo "$OUTPUT" | grep "Passed:" | head -1 | awk '{print $2}' | cut -d/ -f1)
    TOTAL=$(echo "$OUTPUT" | grep "Passed:" | head -1 | awk '{print $2}' | cut -d/ -f2)
    
    if [ -z "$PASSED" ]; then
        # Stage likely crashed
        CRASHED="$CRASHED $stage"
        PCT="CRASH"
    else
        PCT=$(python3 -c "print(f'{$PASSED/$TOTAL*100:.0f}')" 2>/dev/null || echo "$((PASSED * 100 / TOTAL))%")
        TOTAL_PASSED=$((TOTAL_PASSED + PASSED))
        TOTAL_TESTS=$((TOTAL_TESTS + TOTAL))
    fi
    
    echo "| $stage | $STAGE_PATH | $PASSED | $TOTAL | $PCT |" >> "$REPORT"
done

echo "" >> "$REPORT"
echo "## Summary" >> "$REPORT"
echo "" >> "$REPORT"
echo "**Total**: $TOTAL_PASSED / $TOTAL_TESTS passed" >> "$REPORT"
if [ -n "$CRASHED" ]; then
    echo "**Crashed stages**:$CRASHED" >> "$REPORT"
fi
echo "" >> "$REPORT"
echo "Report saved to $REPORT"
cat "$REPORT"
