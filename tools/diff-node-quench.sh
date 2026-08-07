#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=${1:-"$root/tests/node/test/parallel"}
output=${2:-"$root/target/compat/differential.json"}
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}
binary=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
parallel_sides=${QUENCH_DIFF_PARALLEL_SIDES:-1}

sha256_file() {
  node - "$1" <<'NODE'
const crypto = require("crypto");
const fs = require("fs");
const [file] = process.argv.slice(2);
process.stdout.write(crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex"));
NODE
}

case "$parallel_sides" in
  0|1) ;;
  *)
    echo "QUENCH_DIFF_PARALLEL_SIDES must be 0 or 1" >&2
    exit 2
    ;;
esac

if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi

binary_sha256=$(sha256_file "$binary")
comparator_sha256=$(sha256_file "$root/tools/diff-node-quench.sh")
node_runner_sha256=$(sha256_file "$root/tools/run-node-fixture.cjs")

if [ -n "${QUENCH_NODE_DIFF_FINGERPRINT_FILE:-}" ]; then
  if [ ! -r "$QUENCH_NODE_DIFF_FINGERPRINT_FILE" ]; then
    echo "differential fingerprint file is not readable: $QUENCH_NODE_DIFF_FINGERPRINT_FILE" >&2
    exit 2
  fi
  fingerprint=$(cat "$QUENCH_NODE_DIFF_FINGERPRINT_FILE")
else
  fingerprint=$(QUENCH_NODE_BIN="$binary" node "$root/tools/compat-fingerprint.cjs" "$root" "$dir")
fi

if [ ! -d "$dir" ] && [ ! -f "$dir" ]; then
  echo "fixture path does not exist: $dir" >&2
  exit 2
fi

mkdir -p "$(dirname -- "$output")"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/quench-diff.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
if [ -f "$dir" ]; then
  printf '%s\n' "$dir" >"$tmp/files"
else
  find "$dir" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) | sort >"$tmp/files"
fi

run_with_timeout() {
  command_name=$1
  command_file=$2
  shift 2
  if command -v timeout >/dev/null 2>&1; then
    timeout --kill-after=2 "$timeout_seconds" "$@" >"$tmp/$command_file.out" 2>"$tmp/$command_file.err"
  elif command -v gtimeout >/dev/null 2>&1; then
    gtimeout --kill-after=2 "$timeout_seconds" "$@" >"$tmp/$command_file.out" 2>"$tmp/$command_file.err"
  else
    perl -e 'alarm shift; exec @ARGV' "$timeout_seconds" "$@" >"$tmp/$command_file.out" 2>"$tmp/$command_file.err"
  fi
}

normalize() {
  file=$1
  sed \
    -e "s|$root|<ROOT>|g" \
    -e 's|/[Tt]mp/[A-Za-z0-9_.-]*/|<TMP>/|g' \
    -e 's|\\r$||' "$file"
}

now_ms() {
  perl -MTime::HiRes=time -e 'printf "%.0f\n", time * 1000'
}

printf '%s\n' '{"schema":2,"tool":"diff-node-quench","results":[' >"$output"
first=true
total=0
matched=0
node_failed=0
quench_failed=0
different=0

while IFS= read -r file; do
  total=$((total + 1))
  fixture_started_ms=$(now_ms)
  relative=${file#"$root/"}
  fixture_name=$(basename -- "$file")
  prefix_name=${fixture_name#test-}
  prefix=${prefix_name%%-*}
  [ "$prefix_name" = "$fixture_name" ] || [ "$prefix" = "$prefix_name" ] && prefix="unprefixed"
  key=$(printf '%s' "$relative" | cksum | awk '{print $1}')
  fixture_flags=$(sed -nE 's/^[[:space:]]*\/\/[[:space:]]*Flags:[[:space:]]*(.*)$/\1/p' "$file" | head -n 1)

  node_status=0
  quench_status=0
  if [ "$parallel_sides" -eq 1 ]; then
    run_with_timeout node "node-$key" env NODE_NO_WARNINGS=1 TEST_SERIAL_ID="node-$key" node "$root/tools/run-node-fixture.cjs" \
      "$file" &
    node_pid=$!
    run_with_timeout quench "quench-$key" env TEST_SERIAL_ID="quench-$key" "$binary" $fixture_flags "$file" &
    quench_pid=$!
    set +e
    wait "$node_pid"
    node_status=$?
    node_finished_ms=$(now_ms)
    wait "$quench_pid"
    quench_status=$?
    quench_finished_ms=$(now_ms)
    set -e
  else
    run_with_timeout node "node-$key" env NODE_NO_WARNINGS=1 TEST_SERIAL_ID="node-$key" node "$root/tools/run-node-fixture.cjs" \
      "$file" || node_status=$?
    node_finished_ms=$(now_ms)
    run_with_timeout quench "quench-$key" env TEST_SERIAL_ID="quench-$key" "$binary" $fixture_flags "$file" || quench_status=$?
    quench_finished_ms=$(now_ms)
  fi
  fixture_finished_ms=$(now_ms)

  node_out=$(normalize "$tmp/node-$key.out")
  node_err=$(normalize "$tmp/node-$key.err")
  quench_out=$(normalize "$tmp/quench-$key.out")
  quench_err=$(normalize "$tmp/quench-$key.err")
  reporter_suppressed=false
  if rg -q "require\(['\"]node:test['\"]\)|require\(['\"]test['\"]\)" "$file" \
    && [ "$node_status" -eq 0 ] && [ "$quench_status" -eq 0 ] \
    && [ -n "$node_out" ] && [ -z "$quench_out" ]; then
    reporter_suppressed=true
  fi

  diagnostic=$quench_err
  [ -n "$diagnostic" ] || diagnostic=$node_err
  [ -n "$diagnostic" ] || diagnostic=$quench_out
  [ -n "$diagnostic" ] || diagnostic=$node_out
  signature_detail=$(printf '%s\n' "$diagnostic" | sed -n '1p' | sed -E 's/:[0-9]+$//; s/[[:space:]]+/ /g')
  node_environment_limited=false
  node_environment_reason=""
  if [ "$node_status" -ne 0 ]; then
    if printf '%s\n' "$node_err" | rg -q "EPERM: operation not permitted.*listen|require is not defined in ES module scope|globalThis\.gc is not a function|MODULE_NOT_FOUND"; then
      node_environment_limited=true
      node_environment_reason="node-fixture-runner-or-host-environment"
    fi
  fi

  printf '%s' "$node_out" >"$tmp/node-$key.normalized.out"
  printf '%s' "$node_err" >"$tmp/node-$key.normalized.err"
  printf '%s' "$quench_out" >"$tmp/quench-$key.normalized.out"
  printf '%s' "$quench_err" >"$tmp/quench-$key.normalized.err"

  category=match
  if [ "$node_status" -eq 124 ] || [ "$quench_status" -eq 124 ]; then
    category=timeout
    [ "$node_status" -eq 124 ] && node_failed=$((node_failed + 1))
    [ "$quench_status" -eq 124 ] && quench_failed=$((quench_failed + 1))
  elif [ "$node_status" -ne 0 ] && [ "$quench_status" -ne 0 ]; then
    category=both-failed
    node_failed=$((node_failed + 1))
    quench_failed=$((quench_failed + 1))
  elif [ "$node_status" -ne 0 ]; then
    category=node-failed
    node_failed=$((node_failed + 1))
  elif [ "$quench_status" -ne 0 ]; then
    category=quench-failed
    quench_failed=$((quench_failed + 1))
  elif [ "$reporter_suppressed" = true ]; then
    matched=$((matched + 1))
  elif [ "$node_out" != "$quench_out" ] || [ "$node_err" != "$quench_err" ]; then
    category=output-mismatch
    different=$((different + 1))
  else
    matched=$((matched + 1))
  fi

  if [ "$category" != match ] && [ "$category" != output-mismatch ]; then
    different=$((different + 1))
  fi

  if [ "$first" = false ]; then
    printf '%s\n' ',' >>"$output"
  fi
  first=false
  DIFF_RELATIVE="$relative" DIFF_CATEGORY="$category" \
  DIFF_NODE_STATUS="$node_status" DIFF_QUENCH_STATUS="$quench_status" \
  DIFF_NODE_ENVIRONMENT_LIMITED="$node_environment_limited" \
  DIFF_NODE_ENVIRONMENT_REASON="$node_environment_reason" \
  DIFF_FIXTURE_DURATION_MS="$((fixture_finished_ms - fixture_started_ms))" \
  DIFF_NODE_DURATION_MS="$((node_finished_ms - fixture_started_ms))" \
  DIFF_QUENCH_DURATION_MS="$((quench_finished_ms - fixture_started_ms))" \
    DIFF_FIXTURE_STARTED_MS="$fixture_started_ms" \
    DIFF_FIXTURE_FINISHED_MS="$fixture_finished_ms" \
    DIFF_NODE_FINISHED_MS="$node_finished_ms" \
    DIFF_QUENCH_FINISHED_MS="$quench_finished_ms" \
    DIFF_WORKER_PID="$$" \
    DIFF_PREFIX="$prefix" DIFF_SIGNATURE="$category|$node_status|$quench_status|$signature_detail" \
    DIFF_REPORTER_SUPPRESSED="$reporter_suppressed" \
    DIFF_NODE_OUT_FILE="$tmp/node-$key.normalized.out" \
    DIFF_NODE_ERR_FILE="$tmp/node-$key.normalized.err" \
    DIFF_QUENCH_OUT_FILE="$tmp/quench-$key.normalized.out" \
    DIFF_QUENCH_ERR_FILE="$tmp/quench-$key.normalized.err" \
    node -e '
      const fs = require("fs");
      const env = process.env;
      const read = (name) => fs.readFileSync(env[name], "utf8");
      const structuredError = (stdout, stderr, status) => {
        const text = `${stderr}\n${stdout}`;
        const name = text.match(/(?:^|[\n])(?:[A-Za-z0-9_./-]+: )?([A-Za-z][A-Za-z0-9]+Error|Error):/)?.[1] || null;
        const codeMatch = text.match(/(?:code|error code)[:=]\s*["]?([A-Z][A-Z0-9_]+)|\b(ERR_[A-Z0-9_]+|E[A-Z0-9]{2,})\b/);
        const code = codeMatch?.[1] || codeMatch?.[2] || null;
        const location = text.match(/(?:at |^)([^\n()]+):(\d+)(?::(\d+))?/m);
        const callbackMatch = text.match(/Callback\s+(\d+):\s+expected\s+(\d+)\s+calls?,\s+got\s+(\d+)/i);
        const frames = [...text.matchAll(/(?:^|\n)\s*at\s+([^\n]+)/g)]
          .map((match) => match[1].trim())
          .slice(0, 12);
        const phase = /callback|mustCall|once\(/i.test(text) ? "callback" :
          /promise|async|await/i.test(text) ? "promise" :
          /cleanup|close|destroy|exit/i.test(text) ? "cleanup" :
          status === 124 ? "timeout" : status ? "process" : null;
        const firstLine = text.split(/\r?\n/).find((line) => line.trim())?.trim() || null;
        return status || firstLine ? {
          name,
          code,
          message: firstLine,
          file: location?.[1]?.trim() || null,
          line: location ? Number(location[2]) : null,
          column: location?.[3] ? Number(location[3]) : null,
          callback_index: callbackMatch ? Number(callbackMatch[1]) : null,
          callback_expected: callbackMatch ? Number(callbackMatch[2]) : null,
          callback_actual: callbackMatch ? Number(callbackMatch[3]) : null,
          phase,
          frames,
        } : null;
      };
      const nodeStdout = read("DIFF_NODE_OUT_FILE");
      const nodeStderr = read("DIFF_NODE_ERR_FILE");
      const quenchStdout = read("DIFF_QUENCH_OUT_FILE");
      const quenchStderr = read("DIFF_QUENCH_ERR_FILE");
      const result = {
        fixture: env.DIFF_RELATIVE,
        prefix: env.DIFF_PREFIX,
        category: env.DIFF_CATEGORY,
        comparison: env.DIFF_REPORTER_SUPPRESSED === "true" ? "node-test-reporter-suppressed" : "exact-output",
        signature: env.DIFF_SIGNATURE,
        duration_ms: Number(env.DIFF_FIXTURE_DURATION_MS),
        node_duration_ms: Number(env.DIFF_NODE_DURATION_MS),
        quench_duration_ms: Number(env.DIFF_QUENCH_DURATION_MS),
        fixture_started_ms: Number(env.DIFF_FIXTURE_STARTED_MS),
        fixture_finished_ms: Number(env.DIFF_FIXTURE_FINISHED_MS),
        node_finished_ms: Number(env.DIFF_NODE_FINISHED_MS),
        quench_finished_ms: Number(env.DIFF_QUENCH_FINISHED_MS),
        worker_pid: Number(env.DIFF_WORKER_PID),
        fixture_started_at: new Date(Number(env.DIFF_FIXTURE_STARTED_MS)).toISOString(),
        fixture_finished_at: new Date(Number(env.DIFF_FIXTURE_FINISHED_MS)).toISOString(),
        node_timed_out: Number(env.DIFF_NODE_STATUS) === 124,
        quench_timed_out: Number(env.DIFF_QUENCH_STATUS) === 124,
        node_environment_limited: env.DIFF_NODE_ENVIRONMENT_LIMITED === "true",
        node_environment_reason: env.DIFF_NODE_ENVIRONMENT_REASON || null,
        node: { status: Number(env.DIFF_NODE_STATUS), stdout: nodeStdout, stderr: nodeStderr, error: structuredError(nodeStdout, nodeStderr, Number(env.DIFF_NODE_STATUS)) },
        quench: { status: Number(env.DIFF_QUENCH_STATUS), stdout: quenchStdout, stderr: quenchStderr, error: structuredError(quenchStdout, quenchStderr, Number(env.DIFF_QUENCH_STATUS)) }
      };
      process.stdout.write(JSON.stringify(result));
    ' >>"$output"
done <"$tmp/files"

printf '%s\n' ']}' >>"$output"
REPORT_FINGERPRINT="$fingerprint" REPORT_STARTED_AT="$started_at" \
REPORT_BINARY="$binary" REPORT_FIXTURE_ROOT="$dir" \
REPORT_PARALLEL_SIDES="$parallel_sides" \
REPORT_BINARY_SHA256="$binary_sha256" \
REPORT_COMPARATOR_SHA256="$comparator_sha256" \
REPORT_NODE_RUNNER_SHA256="$node_runner_sha256" \
REPORT_GIT_COMMIT="$(git -C "$root" rev-parse HEAD 2>/dev/null || true)" \
  node - "$output" "$root" <<'NODE'
const fs = require("fs");
const path = require("path");

const [output, root] = process.argv.slice(2);
const report = JSON.parse(fs.readFileSync(output, "utf8"));
const fingerprint = JSON.parse(process.env.REPORT_FINGERPRINT);
report.schema = 2;
report.started_at = process.env.REPORT_STARTED_AT;
report.finished_at = new Date().toISOString();
report.timeout_seconds = Number(process.env.QUENCH_NODE_TEST_TIMEOUT_SECONDS || 30);
report.parallel_sides = process.env.REPORT_PARALLEL_SIDES === "1";
report.node_version = process.version;
report.quench_binary = process.env.REPORT_BINARY;
report.quench_binary_sha256 = process.env.REPORT_BINARY_SHA256;
report.comparator_sha256 = process.env.REPORT_COMPARATOR_SHA256;
report.node_runner_sha256 = process.env.REPORT_NODE_RUNNER_SHA256;
report.git_commit = process.env.REPORT_GIT_COMMIT || null;
report.fixture_root = path.relative(root, process.env.REPORT_FIXTURE_ROOT) || ".";
report.fingerprints = {
  source_digest: fingerprint.source_digest,
  fixture_digest: fingerprint.fixture_digest,
  focused_digest: fingerprint.focused_digest,
  ownership_digest: fingerprint.ownership_digest,
  binary_sha256: process.env.REPORT_BINARY_SHA256,
  comparator_sha256: process.env.REPORT_COMPARATOR_SHA256,
  node_runner_sha256: process.env.REPORT_NODE_RUNNER_SHA256,
  timeout_seconds: Number(process.env.QUENCH_NODE_TEST_TIMEOUT_SECONDS || 30),
  parallel_sides: process.env.REPORT_PARALLEL_SIDES === "1",
};
fs.writeFileSync(output, `${JSON.stringify(report)}\n`);
NODE
printf 'fixtures=%s matched=%s different=%s node_failed=%s quench_failed=%s results=%s\n' \
  "$total" "$matched" "$different" "$node_failed" "$quench_failed" "$output"
