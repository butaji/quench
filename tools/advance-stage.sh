#!/bin/bash
# Check if a stage has reached 100% and update index.json.
# Usage: TEST262_STAGE=16 bash tools/advance-stage.sh
#
# Runs the stage, and if it passes 100%, updates tasks/index.json
# to mark it as done and advances current_stage.

set -e
cd "$(dirname "$0")/.."

STAGE=${TEST262_STAGE:-$(python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])")}

echo "Checking Stage $STAGE..."

OUTPUT=$(TEST262_STAGE=$STAGE timeout 60 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture 2>&1 || true)

if echo "$OUTPUT" | grep -q "ALL STAGES COMPLETE"; then
    echo "✅ Stage $STAGE is 100%!"
    
    # Update index.json
    python3 -c "
import json
with open('tasks/index.json') as f:
    d = json.load(f)
for s in d['stages']:
    if s['id'] == $STAGE:
        s['status'] = 'done'
        break
if d['current_stage'] == $STAGE:
    # Advance to next pending stage
    for s in d['stages']:
        if s['status'] != 'done':
            d['current_stage'] = s['id']
            print(f'Advanced current_stage to {s[\"id\"]} ({s[\"path\"]})')
            break
with open('tasks/index.json', 'w') as f:
    json.dump(d, f, indent=2)
    f.write('\n')
print('index.json updated')
"
else
    PASSED=$(echo "$OUTPUT" | grep -oP 'Stage \d+: \K\d+/\d+' || echo "?")
    echo "❌ Stage $STAGE not yet 100% (${PASSED})"
fi
