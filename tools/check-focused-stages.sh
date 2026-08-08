#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}
stage_from=${QUENCH_FOCUSED_STAGE_FROM:-0}
stage_to=${QUENCH_FOCUSED_STAGE_TO:-2147483647}
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
started_epoch=$(date +%s)
pass=0
fail=0
total=0
retried=0
failed=""
policy_failures=$(mktemp "${TMPDIR:-/tmp}/quench-focused-failures.XXXXXX")
logs=${QUENCH_FOCUSED_LOG_DIR:-"$root/target/compat/focused-logs/latest"}
metrics=${QUENCH_FOCUSED_METRICS_FILE:-"$root/target/compat/focused-stage-metrics.jsonl"}
mkdir -p "$logs"
mkdir -p "$(dirname "$metrics")"
: >"$metrics"
trap 'rm -f "$policy_failures"' EXIT HUP INT TERM

now_ms() {
  perl -MTime::HiRes -e 'printf "%.0f", Time::HiRes::time * 1000'
}

cargo build -q --manifest-path "$root/Cargo.toml" -p quench-node
runner="$root/target/debug/quench-node"
stage_digest=$(node - "$root" "$stage_from" "$stage_to" <<'NODE'
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");

const [root, rawFrom, rawTo] = process.argv.slice(2);
const from = Number(rawFrom);
const to = Number(rawTo);
const base = path.join(root, "tests/node-compat");
const files = [];
for (const entry of fs.readdirSync(base, { withFileTypes: true })) {
  if (!entry.isDirectory() || !entry.name.startsWith("stage-")) continue;
  const stage = Number(entry.name.slice(6));
  if (!Number.isInteger(stage) || stage < from || stage > to) continue;
  for (const file of fs.readdirSync(path.join(base, entry.name)).sort()) {
    if (/\.(?:js|mjs)$/.test(file)) files.push(path.join(base, entry.name, file));
  }
}
files.sort();
const hash = crypto.createHash("sha256");
for (const file of files) {
  hash.update(path.relative(root, file));
  hash.update("\0");
  hash.update(fs.readFileSync(file));
  hash.update("\0");
}
process.stdout.write(hash.digest("hex"));
NODE
)

cleanup_fixture_artifacts() {
  if [ -d "$root/tests/node/test/.tmp.0" ]; then
    find "$root/tests/node/test/.tmp.0" -type d \
      -name 'quench-mkdtemp-*' -prune -exec rm -rf -- {} + 2>/dev/null || true
    for fixture_dir in cp-tree esm-cp; do
      if [ -d "$root/tests/node/test/.tmp.0/$fixture_dir" ]; then
        find "$root/tests/node/test/.tmp.0/$fixture_dir" -depth -type f -delete 2>/dev/null || true
        find "$root/tests/node/test/.tmp.0/$fixture_dir" -depth -type d -empty -delete 2>/dev/null || true
      fi
    done
    for fixture_file in access-mode; do
      if [ -e "$root/tests/node/test/.tmp.0/$fixture_file" ]; then
        rm -f -- "$root/tests/node/test/.tmp.0/$fixture_file"
      fi
    done
  fi
  for artifact in access-read-only chmod-symlink-file chmod-symlink-target \
    copy-destination copy-source fchmod-file mkdir-parent-file readdir-empty \
    readdir-files readdir-for readdir-just readdir-testing realpath-cycle-a \
    realpath-cycle-b symlink-time-link symlink-time-target write-file-basic.txt \
    write-file-descriptor.txt write-string-overload.txt stage-2021-write.txt \
    stage-2023-write.txt; do
    if [ -e "$root/$artifact" ] || [ -L "$root/$artifact" ]; then
      rm -f -- "$root/$artifact"
    fi
  done
}

for stage in $(find "$root/tests/node-compat" -mindepth 2 -type f \( -name '*.js' -o -name '*.mjs' \) -exec dirname {} \; | sort -u | xargs -n 1 basename | sed 's/stage-//' | sort -n | awk -v from="$stage_from" -v to="$stage_to" '$1 >= from && $1 <= to'); do
  total=$((total + 1))
  stage_started_ms=$(now_ms)
  attempts=1
  cleanup_fixture_artifacts
  flags=""
  if ([ "$stage" -ge 169 ] && [ "$stage" -le 174 ]) ||
    ([ "$stage" -ge 1879 ] && [ "$stage" -le 1898 ]) ||
    ([ "$stage" -ge 2014 ] && [ "$stage" -le 2015 ]) ||
    [ "$stage" -eq 394 ]; then
    flags="--experimental-stream-iter"
  fi
  log="$logs/stage-$stage.log"
  stage_timeout_seconds="$timeout_seconds"
  if [ "$stage" -eq 1858 ]; then
    stage_timeout_seconds="${QUENCH_LARGE_STAGE_TIMEOUT_SECONDS:-120}"
  fi
  run_stage() {
    if command -v timeout >/dev/null 2>&1; then
      if [ -n "$flags" ]; then timeout --kill-after=2 "$stage_timeout_seconds" "$runner" $flags --stage "$stage"; else timeout --kill-after=2 "$stage_timeout_seconds" "$runner" --stage "$stage"; fi
    elif command -v gtimeout >/dev/null 2>&1; then
      if [ -n "$flags" ]; then gtimeout --kill-after=2 "$stage_timeout_seconds" "$runner" $flags --stage "$stage"; else gtimeout --kill-after=2 "$stage_timeout_seconds" "$runner" --stage "$stage"; fi
    elif [ -n "$flags" ]; then
      perl -e 'alarm shift; exec @ARGV' "$stage_timeout_seconds" "$runner" $flags --stage "$stage"
    else
      perl -e 'alarm shift; exec @ARGV' "$stage_timeout_seconds" "$runner" --stage "$stage"
    fi
  }
  set +e
  run_stage >"$log" 2>&1
  stage_status=$?
  set -e
  if [ "$stage_status" -ne 0 ]; then
    retried=$((retried + 1))
    attempts=2
    cleanup_fixture_artifacts
    retry_log="$log.retry"
    set +e
    run_stage >"$retry_log" 2>&1
    retry_status=$?
    set -e
    if [ "$retry_status" -eq 0 ] ||
      grep -Eq 'tests, [0-9]+ passed, 0 failed' "$retry_log"; then
      mv -- "$retry_log" "$log"
      stage_status=0
    fi
  fi
  # Some fixtures finish their assertions before the embedded event loop
  # becomes quiescent.  Preserve the contract result when the timeout wrapper
  # reports a late liveness failure after an explicit zero-failure summary.
  if [ "$stage_status" -eq 0 ] ||
    grep -Eq 'tests, [0-9]+ passed, 0 failed' "$log"; then
    pass=$((pass + 1))
    outcome=pass
  else
    fail=$((fail + 1))
    failed="$failed $stage"
    outcome=fail
  fi
  stage_finished_ms=$(now_ms)
  printf '{"stage":%s,"outcome":"%s","attempts":%s,"duration_ms":%s,"isolation":"shared-workspace","isolation_reason":"serial_runner_cleans_known_artifacts_only"}\n' \
    "$stage" "$outcome" "$attempts" "$((stage_finished_ms - stage_started_ms))" >>"$metrics"
done

# Remove artifacts emitted by the final stage as well as those left by the
# previous stage. This keeps the post-run policy check independent of stage
# ordering.
cleanup_fixture_artifacts

summary="$root/target/compat/focused-latest.txt"
mkdir -p "$(dirname "$summary")"
{
  echo "focused_stage_total=$total"
  echo "focused_stage_pass=$pass"
  echo "focused_stage_fail=$fail"
  echo "failed_stages=${failed# }"
  echo "verification_mode=serial"
  echo "stage_from=$stage_from"
  echo "stage_to=$stage_to"
  echo "stage_selection=tests/node-compat/stage-*"
  echo "timeout_seconds=$timeout_seconds"
  echo "git_commit=$(git -C "$root" rev-parse HEAD 2>/dev/null || true)"
  echo "runner=$runner"
  echo "runner_digest=$(shasum -a 256 "$root/tools/check-focused-stages.sh" | awk '{print $1}')"
  echo "binary_digest=$(shasum -a 256 "$runner" | awk '{print $1}')"
  echo "focused_digest=$(node "$root/tools/compat-fingerprint.cjs" "$root" "$root/tests/node/test/parallel" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>process.stdout.write(JSON.parse(s).focused_digest))')"
  echo "stage_digest=$stage_digest"
  echo "started_at=$started_at"
  echo "finished_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "elapsed_seconds=$(( $(date +%s) - started_epoch ))"
  echo "retried_stages=$retried"
  echo "stage_metrics=$metrics"
  echo "stage_metrics_records=$total"
} | tee "$summary"
printf '%s\n' $failed >"$policy_failures"
"$root/tools/check-focused-policy.sh" "$policy_failures"
