#!/usr/bin/env bash
set -u
runner=/tmp/tod20-v164-session-target/release/run-parallel
parallel=/workspace/work/node-stable-release/node-v26.12.1/test/parallel
names=(
  test-crypto-kem-shared-key.js
  test-crypto-key-reuse-concurrent.js
  test-crypto-keyobject-clone-transfer.js
  test-crypto-worker-thread.js
  test-webcrypto-cryptokey-workers.js
  test-webcrypto-rsa-shared-key.js
  test-worker-message-channel-sharedarraybuffer.js
  test-worker-workerdata-sharedarraybuffer.js
  test-worker-message-channel.js
  test-worker-message-port-transfer-terminate.js
  test-worker-workerdata-messageport.js
)
for name in "${names[@]}"; do
  stem=${name%.js}
  "$runner" --all --parallel-dir "$parallel" --filter "$name" --jobs 1 --timeout-secs 60 \
    --results "work/tod20-v377-worker-transfer-ablation-$stem-results.json" \
    --checkpoint "work/tod20-v377-worker-transfer-ablation-$stem-checkpoint.jsonl" \
    > "work/tod20-v377-worker-transfer-ablation-$stem.log" 2>&1 || true
done
