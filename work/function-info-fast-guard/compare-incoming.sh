#!/bin/bash
set -euo pipefail
cd /workspace/quench
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/sdk-primitives-bin/quench-node --quench-peer /workspace/quench/work/function-info-before-bin/quench-node --out work/function-info-fast-guard/incoming-fixed-work.json > work/function-info-fast-guard/incoming-fixed-work.log 2>&1
