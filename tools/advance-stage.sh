#!/bin/bash
# Check if a stage has reached 100% and update index.json.
# Usage: TEST262_STAGE=16 bash tools/advance-stage.sh
#
# Runs the stage, and if it passes 100%, updates tasks/index.json
# to mark it as done and advances current_stage.

set -e
cd "$(dirname "$0")/.."

STAGE=$(bash tools/current-stage.sh)

echo "Checking Stage $STAGE..."

# Per-stage timeout scaled to its test count: 10s + 0.2s/test, min 120s.
STAGE_COUNT=$(bash tools/stage-count.sh "$STAGE")
STAGE_TIMEOUT=$((10 + STAGE_COUNT / 5))
[ "$STAGE_TIMEOUT" -lt 120 ] && STAGE_TIMEOUT=120

if command -v timeout >/dev/null 2>&1; then
    OUTPUT=$(TEST262_STAGE=$STAGE timeout "$STAGE_TIMEOUT" cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1 || true)
else
    OUTPUT=$(TEST262_STAGE=$STAGE cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1 || true)
fi

# Success requires the exact footer AND zero skipped tests (the footer is
# suppressed when anything is skipped, but belt-and-braces for stale runners).
if echo "$OUTPUT" | grep -q "ALL STAGES COMPLETE" && ! echo "$OUTPUT" | grep -qE "skipped [1-9]|[1-9][0-9]* skipped"; then
    echo "✅ Stage $STAGE is 100%!"

    # Update index.json
    TEST_STAGE=$STAGE python3 - <<'PY'
import json
import os

with open('tasks/index.json') as f:
    d = json.load(f)

stage = int(os.environ['TEST_STAGE'])
for s in d['stages']:
    if s['id'] == stage:
        s['status'] = 'done'
        break

if d['current_stage'] == stage:
    for s in d['stages']:
        if s['status'] != 'done':
            d['current_stage'] = s['id']
            print(f"Advanced current_stage to {s['id']} ({s['path']})")
            break

with open('tasks/index.json', 'w') as f:
    json.dump(d, f, indent=2)
    f.write('\n')

print('index.json updated')
PY
else
    # Portable (BSD/macOS-safe) extraction of the "Stage N: P/T" tally.
    PASSED=$(echo "$OUTPUT" | grep -oE 'Stage [0-9]+: [0-9]+/[0-9]+' | head -1 | grep -oE '[0-9]+/[0-9]+' || echo "?")
    echo "❌ Stage $STAGE not yet 100% (${PASSED:-?})"
    if echo "$OUTPUT" | grep -qE "skipped [1-9]|[1-9][0-9]* skipped"; then
        echo "   (skipped tests present — stage cannot advance)"
    fi
fi
