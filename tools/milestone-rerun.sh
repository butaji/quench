#!/usr/bin/env bash
# Rerun a failing test for a stage with full diagnostics.
#
# Usage:
#   bash tools/milestone-rerun.sh                # use current stage
#   bash tools/milestone-rerun.sh --stage 34      # use explicit stage
#   bash tools/milestone-rerun.sh --test tests/... # rerun explicit test file

set -euo pipefail

STAGE=""
TEST=""

while [[ ${#} -gt 0 ]]; do
  case "${1:-}" in
    --stage)
      STAGE="$2"
      shift 2
      ;;
    --test)
      TEST="$2"
      shift 2
      ;;
    -h|--help)
      sed -n '1,200p' "$0"
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$STAGE" ]]; then
  STAGE="${TEST262_STAGE:-$(python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])")}" 
fi

if [[ -z "$TEST" ]]; then
  OUTFILE="$(mktemp)"
  if ! TEST262_STAGE="$STAGE" TEST262_DIGEST=1 TEST262_QUICK=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture > "$OUTFILE" 2>&1; then
    :
  fi

  TEST="$(python3 - "$OUTFILE" <<'PY'
import pathlib
import re
import sys
text = pathlib.Path(sys.argv[1]).read_text(errors='ignore')
patterns = [
    re.compile(r'"sample_paths"\s*:\s*\[\s*"([^\"]+\.js)"', re.DOTALL),
    re.compile(r'"path"\s*:\s*"([^\"]+\.js)"', re.DOTALL),
    re.compile(r'"test"\s*:\s*"([^\"]+\.js)"', re.DOTALL),
    re.compile(r'"file"\s*:\s*"([^\"]+\.js)"', re.DOTALL),
    re.compile(r"FAILED [^\n]*tests/.+?\.js"),
]

candidates = []
for pat in patterns:
    if pat.pattern.startswith('FAILED'):
        for m in pat.finditer(text):
            mm = re.search(r'(tests/.+\.js)', m.group(0))
            if mm:
                cand = mm.group(1)
                if cand not in candidates:
                    candidates.append(cand)
        continue

    for m in pat.finditer(text):
        cand = m.group(1)
        if cand not in candidates:
            candidates.append(cand)

if not candidates:
    raise SystemExit(1)
print(candidates[0])
PY
  )"
  rm -f "$OUTFILE"
fi

if [[ -z "$TEST" ]]; then
  echo "no failing test found for stage $STAGE" >&2
  exit 1
fi

echo "[milestone-rerun] Running diagnostics for ${TEST}"
cargo run --bin run-test -- --show-script "$TEST"
