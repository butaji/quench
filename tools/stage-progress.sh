#!/usr/bin/env bash
# Measure a single stage's pass/fail/skipped quickly.
# Usage:
#   bash tools/stage-progress.sh <stage>
#   bash tools/stage-progress.sh 44
# Captures only passed/failed/skipped counts so it is cheap to run between
# iterations during a debugging cycle.
set -euo pipefail

STAGE="${1:-}"

if [[ -z "$STAGE" ]]; then
    STAGE="$(bash tools/current-stage.sh)"
fi

TEST262_DIGEST=1 timeout 600 env TEST262_STAGE="$STAGE" \
    cargo nextest run -p quench-test262 --test test262 \
    --profile test262 -E 'test(test262_staged)' \
    --run-ignored all --no-capture 2>&1 \
    | python3 -c '
import json
import re
import sys

text = sys.stdin.read()
# The digest JSON is on a single line with a "{" prefix; isolate it.
m = re.search(r"\{[^{}]*\"passed\"[^{}]*\"failed\"[^{}]*\}", text, re.DOTALL)
if not m:
    m = re.search(r"\{.*?\"path\".*?\}", text, re.DOTALL)
if not m:
    print("no digest found")
    sys.exit(1)
data = json.loads(m.group(0))
stage = data["stage"]
path = data["path"]
passed = data["passed"]
failed = data["failed"]
total = data["total"]
skipped = data["skipped"]
print("stage {} {}: {}/{} passed, {} failed, {} skipped".format(
    stage, path, passed, total, failed, skipped))
'
