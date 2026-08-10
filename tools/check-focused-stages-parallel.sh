#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
# The compatibility fixtures intentionally use relative paths in the repository
# root. Keep the authoritative default serial; callers may opt into parallel
# execution only when their fixture set is isolated.
jobs=${QUENCH_NODE_STAGE_JOBS:-${JOBS:-1}}
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}
stage_from=${QUENCH_FOCUSED_STAGE_FROM:-0}
stage_to=${QUENCH_FOCUSED_STAGE_TO:-2147483647}
if [ "$jobs" -eq 1 ]; then
  exec "$root/tools/check-focused-stages.sh"
fi
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
started_epoch=$(date +%s)
failures=$(mktemp)
trap 'rm -f "$failures"' EXIT HUP INT TERM
export root failures
export timeout_seconds

for artifact in access-read-only chmod-symlink-file chmod-symlink-target copy-destination copy-source fchmod-file mkdir-parent-file readdir-empty readdir-files readdir-for readdir-just readdir-testing realpath-cycle-a realpath-cycle-b symlink-time-link symlink-time-target write-file-basic.txt write-file-descriptor.txt write-string-overload.txt; do
  if [ -e "$root/$artifact" ] || [ -L "$root/$artifact" ]; then
    rm -f -- "$root/$artifact"
  fi
done

cargo build -q --manifest-path "$root/Cargo.toml" -p quench-node
runner="$root/target/debug/quench-node"
export runner
logs=${QUENCH_FOCUSED_LOG_DIR:-"$root/target/compat/focused-logs/parallel"}
mkdir -p "$logs"
export logs
metrics=${QUENCH_FOCUSED_METRICS_FILE:-"$root/target/compat/focused-stage-metrics-parallel.jsonl"}
metrics_dir=$(mktemp -d "${TMPDIR:-/tmp}/quench-focused-parallel-metrics.XXXXXX")
trap 'rm -rf "$metrics_dir"; rm -f "$failures"' EXIT HUP INT TERM
export metrics_dir

find "$root/tests/node-compat" -mindepth 2 -type f \( -name '*.js' -o -name '*.mjs' \) -exec dirname {} \; | sort -u | xargs -n 1 basename \
  | sed 's/^stage-//' | awk '/^[0-9]+$/ { print }' | sort -n | while IFS= read -r stage; do
  if [ "$stage" -lt "$stage_from" ] || [ "$stage" -gt "$stage_to" ]; then
    continue
  fi
  printf '%s\n' "$stage"
done >"$logs/stages.list"
total=$(wc -l <"$logs/stages.list" | tr -d ' ')
xargs -n 1 -P "$jobs" sh -c '
  stage=$1
  flags=""
  if ([ "$stage" -ge 169 ] && [ "$stage" -le 174 ]) ||
    ([ "$stage" -ge 1879 ] && [ "$stage" -le 1898 ]) ||
    ([ "$stage" -ge 2014 ] && [ "$stage" -le 2015 ]) ||
    ([ "$stage" -ge 2434 ] && [ "$stage" -le 2436 ]) ||
    ([ "$stage" -ge 2522 ] && [ "$stage" -le 2528 ]) ||
    [ "$stage" -eq 394 ]; then
    flags="--experimental-stream-iter"
  fi
  run_stage() {
    if command -v timeout >/dev/null 2>&1; then
      timeout --kill-after=2 "$timeout_seconds" "$runner" $flags --stage "$stage"
    elif command -v gtimeout >/dev/null 2>&1; then
      gtimeout --kill-after=2 "$timeout_seconds" "$runner" $flags --stage "$stage"
    else
      perl -e "alarm shift; exec \\@ARGV" "$timeout_seconds" "$runner" $flags --stage "$stage"
    fi
  }
  log="$logs/stage-$stage.log"
  stage_started_ms=$(perl -MTime::HiRes -e "printf \"%.0f\", Time::HiRes::time * 1000")
  set +e
  run_stage >"$log" 2>&1
  status=$?
  set -e
  if [ "$status" -ne 0 ] && ! grep -Eq "tests, [0-9]+ passed, 0 failed" "$log"; then
    retry_log="$log.retry"
    set +e
    run_stage >"$retry_log" 2>&1
    retry_status=$?
    set -e
    if [ "$retry_status" -eq 0 ] ||
      grep -Eq "tests, [0-9]+ passed, 0 failed" "$retry_log"; then
      mv -- "$retry_log" "$log"
    else
      rm -f -- "$retry_log"
      printf "%s\\n" "$stage" >>"$failures"
    fi
  fi
  stage_finished_ms=$(perl -MTime::HiRes -e "printf \"%.0f\", Time::HiRes::time * 1000")
  attempts=1
  if [ "$status" -ne 0 ]; then attempts=2; fi
  if grep -Eq "tests, [0-9]+ passed, 0 failed" "$log" || [ "$status" -eq 0 ]; then
    outcome=pass
  else
    outcome=fail
  fi
  printf "{\"stage\":%s,\"outcome\":\"%s\",\"attempts\":%s,\"duration_ms\":%s,\"isolation\":\"shared-workspace\",\"isolation_reason\":\"parallel_workers_share_repository_paths\"}\\n" \
    "$stage" "$outcome" "$attempts" "$((stage_finished_ms - stage_started_ms))" >"$metrics_dir/stage-$stage.json"
' sh <"$logs/stages.list"

fail=$(wc -l <"$failures" | tr -d ' ')
pass=$((total - fail))
if [ -d "$metrics_dir" ]; then
  : >"$metrics"
  for metrics_file in $(find "$metrics_dir" -type f -name 'stage-*.json' | sort -n); do
    cat "$metrics_file" >>"$metrics"
  done
else
  : >"$metrics"
fi
summary="$root/target/compat/focused-latest.txt"
mkdir -p "$(dirname "$summary")"
{
  echo "focused_stage_total=$total"
  echo "focused_stage_pass=$pass"
  echo "focused_stage_fail=$fail"
  echo "failed_stages=$(sort -n "$failures" | tr "\n" " " | sed "s/[[:space:]]*$//")"
  echo "verification_mode=parallel"
  echo "jobs=$jobs"
  echo "stage_from=$stage_from"
  echo "stage_to=$stage_to"
  echo "stage_selection=tests/node-compat/stage-*"
  echo "timeout_seconds=$timeout_seconds"
  echo "git_commit=$(git -C "$root" rev-parse HEAD 2>/dev/null || true)"
  echo "runner=$runner"
  echo "runner_digest=$(shasum -a 256 "$root/tools/check-focused-stages-parallel.sh" | awk '{print $1}')"
  echo "binary_digest=$(shasum -a 256 "$runner" | awk '{print $1}')"
  echo "focused_digest=$(node "$root/tools/compat-fingerprint.cjs" "$root" "$root/tests/node/test/parallel" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>process.stdout.write(JSON.parse(s).focused_digest))')"
  echo "started_at=$started_at"
  echo "finished_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "elapsed_seconds=$(( $(date +%s) - started_epoch ))"
  echo "stage_metrics=$metrics"
  echo "stage_metrics_records=$(wc -l <"$metrics" | tr -d ' ')"
} | tee "$summary"
echo "focused_stage_pass=$pass"
echo "focused_stage_fail=$fail"
echo "failed_stages=$(sort -n "$failures" | tr "\n" " " | sed "s/[[:space:]]*$//")"
"$root/tools/check-focused-policy.sh" "$failures"
[ "$fail" -eq 0 ]
