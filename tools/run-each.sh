#!/bin/bash
# Process-isolated test runner — runs each test in a subprocess to survive crashes.
# Usage: TEST262_STAGE=16 ./tools/run-each.sh
#
# Builds run-test once (release by default), then invokes the binary directly
# in parallel (xargs -P, one worker per CPU). Each test runs under a perl
# watchdog (portable — stock macOS has no GNU timeout) and is killed after
# 15s, matching TEST_TIMEOUT_SECS in src/test262/runner/execute.rs.
#
# Env:
#   TEST262_STAGE=N     stage to run (default: current_stage in tasks/index.json)
#   TEST262_PROFILE=debug|release   build/run profile (default: release)
#   TEST262_JOBS=N      parallel workers (default: all cores)
#   TEST262_GLOB=pat    restrict to matching test paths (smoke runs)
#
# run-test exit codes: 0=pass, 1=fail (error type mismatch), 2=usage error,
# 3=negative wrongly passed (fail), 4=harness/read error (fail), 124=timeout.

set -u
cd "$(dirname "$0")/.."

STAGE=${TEST262_STAGE:-$(python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])")}
PROFILE=${TEST262_PROFILE:-release}
JOBS=${TEST262_JOBS:-$(sysctl -n hw.ncpu 2>/dev/null || nproc)}
TIMEOUT_SECS=15
BIN="target/$PROFILE/run-test"

if [ "$PROFILE" = "release" ]; then
    cargo build --release --bin run-test || exit 1
else
    cargo build --bin run-test || exit 1
fi
[ -x "$BIN" ] || { echo "run-test binary missing: $BIN"; exit 1; }

STAGE_DIR=$(python3 -c "
import json
d = json.load(open('tasks/index.json'))
print(next(s['path'] for s in d['stages'] if s['id'] == $STAGE))
")
TEST_DIR="tests/test262/$STAGE_DIR"
[ -d "$TEST_DIR" ] || { echo "Stage $STAGE directory not found: $TEST_DIR"; exit 1; }

if command -v perl >/dev/null 2>&1; then
    # Watchdog: fork, exec the runner, SIGKILL it after the timeout → 124.
    with_timeout() {
        perl -e '
            my $t = shift;
            my $pid = fork();
            die "fork: $!" unless defined $pid;
            if ($pid == 0) { exec @ARGV; exit 127; }
            local $SIG{ALRM} = sub { kill 9, $pid; waitpid $pid, 0; exit 124; };
            alarm $t;
            waitpid $pid, 0;
            my $s = $?;
            exit($s & 127 ? 128 + ($s & 127) : $s >> 8);
        ' "$TIMEOUT_SECS" "$@"
    }
else
    echo "warning: perl not found — running WITHOUT a per-test timeout" >&2
    with_timeout() { "$@"; }
fi

export BIN TEST_DIR TIMEOUT_SECS
export -f with_timeout

TMPDIR_RUN=$(mktemp -d)
trap "rm -rf '$TMPDIR_RUN'" EXIT
mkdir -p "$TMPDIR_RUN/logs"

run_one() {
    local test="$1"
    local rel="${test#$TEST_DIR/}"
    local log="$TMPDIR_RUN/logs/$(echo "$rel" | tr '/ ' '__').log"
    with_timeout "$BIN" "$test" >"$log" 2>&1
    local code=$?
    case $code in
        0) echo "PASS $rel" ;;
        124) echo "TIMEOUT $rel" ;;
        *) echo "FAIL($code) $rel" ;;
    esac
}
export TMPDIR_RUN
export -f run_one

echo "=== Process-isolated run: Stage $STAGE ($TEST_DIR) [$PROFILE, $JOBS workers] ==="

FIND_ARGS=( "$TEST_DIR" -name "*.js" ! -name "*_FIXTURE.js" )
RESULTS="$TMPDIR_RUN/results.txt"
if [ -n "${TEST262_GLOB:-}" ]; then
    find "${FIND_ARGS[@]}" | grep "$TEST262_GLOB" | sort
else
    find "${FIND_ARGS[@]}" | sort
fi | xargs -P "$JOBS" -I{} bash -c 'run_one "$@"' _ {} > "$RESULTS"

PASSED=$(grep -c "^PASS " "$RESULTS" || true)
FAILED=$(grep -cv "^PASS " "$RESULTS" || true)
TOTAL=$(wc -l < "$RESULTS" | tr -d ' ')

echo ""
grep -v "^PASS " "$RESULTS" | while read -r line; do
    rel="${line#* }"
    log="$TMPDIR_RUN/logs/$(echo "$rel" | tr '/ ' '__').log"
    echo "  $line"
    head -3 "$log" 2>/dev/null | while read -r l; do echo "    $l"; done
done

echo ""
echo "=== Results: $PASSED passed, $FAILED failed ($TOTAL total) ==="
[ "$FAILED" = "0" ]
