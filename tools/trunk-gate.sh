#!/bin/bash
# The trunk gate: test262 and the shared Wasm suite in every execution mode, on the checked-out
# tree. A test that timed out is run again alone before it counts, since a timeout under load
# says nothing about the engine. When every mode passes, the commit gets a pass record in
# `refs/notes/gate` naming its tree, which `tools/trunk-pre-push.sh` requires for a trunk push.
#
# usage: tools/trunk-gate.sh OUT_DIR [MODE...]   (MODE: no-jit | b-jit | default; all three by default)
# env:   CARGO_TARGET_DIR (default target), TEST262_TEST_TIMEOUT_MS (10000), WASM_FILE_TIMEOUT_MS (60000)
set -u
out=${1:?usage: tools/trunk-gate.sh OUT_DIR [MODE...]}; shift
modes=("$@"); [ ${#modes[@]} -eq 0 ] && modes=(no-jit b-jit default)
root=$(git rev-parse --show-toplevel); cd "$root" || exit 2
target=${CARGO_TARGET_DIR:-$root/target}
export TEST262_TEST_TIMEOUT_MS=${TEST262_TEST_TIMEOUT_MS:-10000} WASM_FILE_TIMEOUT_MS=${WASM_FILE_TIMEOUT_MS:-60000}
mkdir -p "$out"
commit=$(git rev-parse HEAD); tree=$(git rev-parse 'HEAD^{tree}')
[ -z "$(git status --porcelain --untracked-files=no)" ] || { echo "gate: the tree has uncommitted changes"; exit 2; }

cargo build --profile experiment -p quench-test262 --bin run-all --bin run-test -p quench-wasm-test --bin run \
  > "$out/build.log" 2>&1 || { echo "gate: build failed ($out/build.log)"; exit 1; }

# The paths of a test262 report's failures other than timeouts, after re-running each timeout alone.
remaining_failures() {
  local report=$1 flag=$2
  python3 - "$report" <<'EOF' | while IFS=$'\t' read -r kind path; do
import json, sys
for failure in json.load(open(sys.argv[1]))['failures']:
    timed_out = (failure.get('reason') or '').startswith('timed_out') or failure.get('outcome') == 'timed_out'
    print(('timeout' if timed_out else 'fail') + '\t' + failure['path'])
EOF
    if [ "$kind" = timeout ] && "$target/experiment/run-test" $flag "tests/test262/test/$path" > /dev/null 2>&1; then
      continue
    fi
    echo "$path"
  done
}

summary=(); failed=0
for mode in "${modes[@]}"; do
  flag=""; [ "$mode" != default ] && flag="--$mode"
  TEST262_DIR=tests/test262 TEST262_REPORT="$out/t262.$mode.json" TEST262_RATCHET="$out/ratchet.$mode.json" \
    "$target/experiment/run-all" $flag > "$out/t262.$mode.log" 2>&1
  [ -f "$out/t262.$mode.json" ] || { echo "gate: test262 $mode left no report"; failed=1; continue; }
  remaining=$(remaining_failures "$out/t262.$mode.json" "$flag")
  count=$(printf '%s' "$remaining" | grep -c . || true)
  echo "test262 $mode: $(tail -1 "$out/t262.$mode.log"); failures after timeout re-runs: $count"
  [ "$count" -eq 0 ] || { printf '%s\n' "$remaining" > "$out/t262.$mode.failures"; failed=1; }
  "$target/experiment/run" $flag --report "$out/wasm.$mode.json" crates/quench-wasm-test/testsuite > "$out/wasm.$mode.log" 2>&1
  wasm=$(tail -2 "$out/wasm.$mode.log" | head -1)
  echo "wasm $mode: $wasm"
  echo "$wasm" | grep -q ' 0 failed' || failed=1
  summary+=("$mode")
done

if [ "$failed" -eq 0 ]; then
  git notes --ref=gate add -f -m "gate pass tree=$tree modes=$(IFS=,; echo "${summary[*]}") at=$(date -u +%FT%TZ)" "$commit"
  echo "gate: pass recorded on $commit (the record stays in this clone; note the tree on the landing issue)"
else
  echo "gate: FAILED on $commit; details in $out"
  exit 1
fi
