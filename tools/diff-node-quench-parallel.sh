#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=${1:-"$root/tests/node/test/parallel"}
output=${2:-"$root/target/compat/differential-parallel.json"}
workers=${QUENCH_DIFF_WORKERS:-8}
from=${QUENCH_DIFF_FROM:-1}
to=${QUENCH_DIFF_TO:-0}
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
heartbeat_seconds=${QUENCH_DIFF_HEARTBEAT_SECONDS:-30}

sha256_file() {
  node - "$1" <<'NODE'
const crypto = require("crypto");
const fs = require("fs");
const [file] = process.argv.slice(2);
process.stdout.write(crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex"));
NODE
}

if ! [ "$workers" -ge 1 ] 2>/dev/null; then
  echo "QUENCH_DIFF_WORKERS must be a positive integer" >&2
  exit 2
fi
if ! [ "$heartbeat_seconds" -ge 0 ] 2>/dev/null; then
  echo "QUENCH_DIFF_HEARTBEAT_SECONDS must be a non-negative integer" >&2
  exit 2
fi
case "$from" in
  ''|*[!0-9]*)
    echo "QUENCH_DIFF_FROM must be a non-negative integer" >&2
    exit 2
    ;;
esac
case "$to" in
  ''|*[!0-9]*)
    echo "QUENCH_DIFF_TO must be a non-negative integer" >&2
    exit 2
    ;;
esac
if [ ! -d "$dir" ] && [ ! -f "$dir" ]; then
  echo "fixture path does not exist: $dir" >&2
  exit 2
fi

mkdir -p "$(dirname -- "$output")"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/quench-diff-parallel.XXXXXX")
cleanup_pid_tree() {
  pid=$1
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    cleanup_pid_tree "$child"
  done
  kill -TERM "$pid" 2>/dev/null || true
}

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  for child in $(pgrep -P "$$" 2>/dev/null || true); do
    cleanup_pid_tree "$child"
  done
  rm -rf "$tmp"
  exit "$status"
}
trap cleanup EXIT HUP INT TERM
mkdir "$tmp/reports"

# Build once before fan-out.  The single-fixture runner still self-builds when
# used directly, but doing that from every worker creates needless Cargo lock
# contention and can leave a partial corpus looking like a successful run.
binary=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi
[ -x "$binary" ] || {
  echo "quench-node binary is not executable: $binary" >&2
  exit 2
}

binary_sha256=$(sha256_file "$binary")
comparator_sha256=$(sha256_file "$root/tools/diff-node-quench.sh")
node_runner_sha256=$(sha256_file "$root/tools/run-node-fixture.cjs")

fingerprint=$(QUENCH_NODE_BIN="$binary" node "$root/tools/compat-fingerprint.cjs" "$root" "$dir")
fingerprint_file="$tmp/fingerprint.json"
printf '%s\n' "$fingerprint" >"$fingerprint_file"

if [ -f "$dir" ]; then
  printf '%s\0' "$dir" >"$tmp/files"
else
  find "$dir" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) -print0 | sort -z >"$tmp/files"
fi
if [ "$to" -gt 0 ] 2>/dev/null || [ "$from" -gt 1 ] 2>/dev/null; then
  tr '\0' '\n' <"$tmp/files" | sed -n "${from},${to:-\$}p" | tr '\n' '\0' >"$tmp/files.slice"
  mv "$tmp/files.slice" "$tmp/files"
fi
expected=$(tr '\0' '\n' <"$tmp/files" | sed '/^$/d' | wc -l | tr -d ' ')

export root tmp binary fingerprint_file
if [ "$expected" -gt 0 ]; then
  xargs -0 -n 1 -P "$workers" sh -c '
    set -eu
    file=$1
    report=$(mktemp "$tmp/reports/result.XXXXXX")
    if QUENCH_NODE_BIN="$binary" \
      QUENCH_NODE_DIFF_FINGERPRINT_FILE="$fingerprint_file" \
      QUENCH_NODE_TEST_TIMEOUT_SECONDS="${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}" \
      "$root/tools/diff-node-quench.sh" "$file" "$report" >"$report.log" 2>&1; then
      exit 0
    fi
    printf "%s\\n" "$file" >"$report.failed"
    exit 1
  ' sh <"$tmp/files" &
  xargs_pid=$!
else
  xargs_pid=
fi
if [ "$heartbeat_seconds" -gt 0 ]; then
  while [ -n "$xargs_pid" ] && kill -0 "$xargs_pid" 2>/dev/null; do
    completed=$(find "$tmp/reports" -type f -name 'result.*' \
      ! -name '*.log' ! -name '*.failed' | wc -l | tr -d ' ')
    failed=$(find "$tmp/reports" -type f -name 'result.*.failed' | wc -l | tr -d ' ')
    elapsed=$(node -e 'const started = Date.parse(process.argv[1]); console.log(Math.max(0, Math.round((Date.now() - started) / 1000)));' "$started_at")
    rate=$(node -e 'const completed = Number(process.argv[1]); const elapsed = Number(process.argv[2]); console.log(elapsed > 0 ? (completed / elapsed).toFixed(2) : "0.00");' "$completed" "$elapsed")
    printf 'progress completed=%s/%s failed_workers=%s elapsed_seconds=%s fixtures_per_second=%s\n' \
      "$completed" "$expected" "$failed" "$elapsed" "$rate" >&2
    if [ "$completed" -ge "$expected" ] && [ "$failed" -eq 0 ]; then
      break
    fi
    sleep "$heartbeat_seconds"
  done
fi
set +e
if [ -n "$xargs_pid" ]; then
  wait "$xargs_pid"
  xargs_status=$?
else
  xargs_status=0
fi
set -e

failed_workers=$(find "$tmp/reports" -type f -name 'result.*.failed' | wc -l | tr -d ' ')
if [ "$xargs_status" -ne 0 ] || [ "$failed_workers" -ne 0 ]; then
  echo "differential run incomplete: worker_failures=$failed_workers" >&2
  find "$tmp/reports" -type f -name 'result.*.failed' -exec sed 's/^/worker_failed=/' {} \; >&2
  exit 1
fi

find "$tmp/reports" -type f -name 'result.*' ! -name '*.log' ! -name '*.failed' \
  | sort >"$tmp/reports.list"

REPORT_OUTPUT="$output" REPORT_LIST="$tmp/reports.list" REPORT_WORKERS="$workers" \
REPORT_TIMEOUT="${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}" REPORT_EXPECTED="$expected" \
REPORT_STARTED_AT="$started_at" REPORT_BINARY="$binary" \
REPORT_PARALLEL_SIDES="${QUENCH_DIFF_PARALLEL_SIDES:-1}" \
REPORT_BINARY_SHA256="$binary_sha256" \
REPORT_COMPARATOR_SHA256="$comparator_sha256" \
REPORT_NODE_RUNNER_SHA256="$node_runner_sha256" \
REPORT_FINGERPRINT="$fingerprint" REPORT_FIXTURE_ROOT="$dir" \
REPORT_GIT_COMMIT="$(git -C "$root" rev-parse HEAD 2>/dev/null || true)" node <<'NODE'
const fs = require("fs");

const output = process.env.REPORT_OUTPUT;
const expected = Number(process.env.REPORT_EXPECTED);
const list = fs.existsSync(process.env.REPORT_LIST)
  ? fs.readFileSync(process.env.REPORT_LIST, "utf8").trim().split("\n").filter(Boolean)
  : [];
const results = [];
let invalidReports = 0;
for (const file of list) {
  try {
    const report = JSON.parse(fs.readFileSync(file, "utf8"));
    results.push(...report.results);
  } catch (error) {
    invalidReports += 1;
    console.error(`invalid worker report ${file}: ${error.message}`);
  }
}
if (invalidReports || results.length !== expected) {
  throw new Error(`incomplete differential merge: expected=${expected} results=${results.length} invalid_reports=${invalidReports}`);
}
const fingerprint = JSON.parse(process.env.REPORT_FINGERPRINT);
results.sort((a, b) => a.fixture.localeCompare(b.fixture));
const finiteValues = (field) => results
  .map((result) => Number(result[field]))
  .filter(Number.isFinite)
  .sort((a, b) => a - b);
const percentile = (values, fraction) => values.length
  ? values[Math.min(values.length - 1, Math.floor((values.length - 1) * fraction))]
  : null;
const fixtureDurations = finiteValues("duration_ms");
const nodeDurations = finiteValues("node_duration_ms");
const quenchDurations = finiteValues("quench_duration_ms");
const intervals = results
  .map((result) => ({
    start: Number(result.fixture_started_ms),
    end: Number(result.fixture_finished_ms),
  }))
  .filter(({ start, end }) => Number.isFinite(start) && Number.isFinite(end) && end >= start);
const events = intervals
  .flatMap(({ start, end }) => [[start, 1], [end, -1]])
  .sort((a, b) => a[0] - b[0] || a[1] - b[1]);
let inFlight = 0;
let maxInFlight = 0;
for (const [, delta] of events) {
  inFlight += delta;
  maxInFlight = Math.max(maxInFlight, inFlight);
}
const criticalPathMs = intervals.length
  ? Math.max(...intervals.map(({ end }) => end)) - Math.min(...intervals.map(({ start }) => start))
  : null;
const sumFixtureDurationMs = intervals.reduce((sum, { start, end }) => sum + end - start, 0);
const finishedAt = new Date().toISOString();
const wallClockMs = Math.max(
  0,
  Date.parse(finishedAt) - Date.parse(process.env.REPORT_STARTED_AT),
);
const summary = {
  schema: 2,
  tool: "diff-node-quench-parallel",
  workers: Number(process.env.REPORT_WORKERS),
  timeout_seconds: Number(process.env.REPORT_TIMEOUT),
  parallel_sides: process.env.REPORT_PARALLEL_SIDES === "1",
  started_at: process.env.REPORT_STARTED_AT,
  finished_at: new Date().toISOString(),
  node_version: process.version,
  quench_binary: process.env.REPORT_BINARY,
  quench_binary_sha256: process.env.REPORT_BINARY_SHA256,
  comparator_sha256: process.env.REPORT_COMPARATOR_SHA256,
  node_runner_sha256: process.env.REPORT_NODE_RUNNER_SHA256,
  git_commit: process.env.REPORT_GIT_COMMIT || null,
  fixture_root: process.env.REPORT_FIXTURE_ROOT,
  fingerprints: {
    source_digest: fingerprint.source_digest,
    fixture_digest: fingerprint.fixture_digest,
    focused_digest: fingerprint.focused_digest,
    ownership_digest: fingerprint.ownership_digest,
    binary_sha256: process.env.REPORT_BINARY_SHA256,
    comparator_sha256: process.env.REPORT_COMPARATOR_SHA256,
    node_runner_sha256: process.env.REPORT_NODE_RUNNER_SHA256,
    timeout_seconds: Number(process.env.REPORT_TIMEOUT),
    parallel_sides: process.env.REPORT_PARALLEL_SIDES === "1",
  },
  audit: {
    node_environment_limited: results.filter((result) => result.node_environment_limited).length,
  },
  telemetry: {
    expected_results: expected,
    actual_results: results.length,
    workers_requested: Number(process.env.REPORT_WORKERS),
    workers_effective: Math.min(Number(process.env.REPORT_WORKERS), expected),
    wall_clock_ms: wallClockMs,
    fixtures_per_second: wallClockMs > 0 ? results.length / (wallClockMs / 1000) : null,
    fixture_duration_ms: {
      p50: percentile(fixtureDurations, 0.5),
      p95: percentile(fixtureDurations, 0.95),
      max: fixtureDurations.at(-1) ?? null,
    },
    node_duration_ms: {
      p50: percentile(nodeDurations, 0.5),
      p95: percentile(nodeDurations, 0.95),
      max: nodeDurations.at(-1) ?? null,
    },
    quench_duration_ms: {
      p50: percentile(quenchDurations, 0.5),
      p95: percentile(quenchDurations, 0.95),
      max: quenchDurations.at(-1) ?? null,
    },
    interval_coverage: intervals.length,
    critical_path_ms: criticalPathMs,
    sum_fixture_duration_ms: sumFixtureDurationMs,
    observed_parallelism: criticalPathMs > 0 ? sumFixtureDurationMs / criticalPathMs : null,
    max_in_flight: maxInFlight,
    unique_worker_processes: new Set(results.map((result) => result.worker_pid).filter(Number.isFinite)).size,
  },
  results,
};
fs.writeFileSync(output, `${JSON.stringify(summary)}\n`);
const counts = new Map();
for (const result of results) counts.set(result.category, (counts.get(result.category) || 0) + 1);
const matched = counts.get("match") || 0;
const different = results.length - matched;
console.log(`fixtures=${results.length} matched=${matched} different=${different} output=${output}`);
for (const [category, count] of [...counts].sort()) console.log(`${category}=${count}`);
console.log(`node_environment_limited=${summary.audit.node_environment_limited}`);
NODE
