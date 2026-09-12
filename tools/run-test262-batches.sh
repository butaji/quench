#!/usr/bin/env bash
set -euo pipefail

# Run the complete sorted Test262 discovery list in restartable, sequential
# batches. Stage metadata is intentionally not used: it does not cover every
# runnable file in the pinned checkout.
batch_size="${TEST262_BATCH_SIZE:-1000}"
batch_count="${TEST262_BATCH_COUNT:-54}"
runner="${TEST262_RUNNER:-target/release/run-all}"
report_dir="${TEST262_BATCH_REPORT_DIR:-target/test262-batches/files}"
mkdir -p "$report_dir"

for ((index = 0; index < batch_count; index += 1)); do
  echo "TEST262 batch ${index}/${batch_count}"
  TEST262_BATCH_SIZE="$batch_size" \
    TEST262_BATCH_INDEX="$index" \
    TEST262_REPORT="$report_dir/${index}.json" \
    "$runner"
done

echo "TEST262 sequential batches complete: ${batch_count} x ${batch_size} (last batch may be partial)"
