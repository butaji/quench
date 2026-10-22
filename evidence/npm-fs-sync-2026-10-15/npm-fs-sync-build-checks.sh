#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "%s\n" "$status" > /workspace/scratch/npm-fs-sync-build-checks.exit' EXIT
source /workspace/quench-build-env.sh
export PATH="/workspace/shared/node-v26.12.0-linux-x64/bin:$PATH"
export CARGO_TARGET_DIR=/workspace/quench/target/npm-file-url CARGO_INCREMENTAL=0
export PKG_CONFIG_PATH=/workspace/shared/icu-dev/root/usr/lib/x86_64-linux-gnu/pkgconfig
cd /workspace/scratch/quench-npm-file-url
run() {
  local label=$1; shift
  local status
  if "$@" > "/workspace/scratch/npm-fs-sync-${label}.log" 2>&1; then status=0; else status=$?; fi
  printf '%s\n' "$status" > "/workspace/scratch/npm-fs-sync-${label}.exit"
  return "$status"
}
run workspace-check cargo check --workspace --all-targets
run fast-core cargo test -p quench-runtime --lib
run cli cargo build --profile iteration -p quench-node --bin quench-node
python3 - <<'PY'
from pathlib import Path
import shutil,hashlib,json,subprocess
root=Path('/workspace/scratch/quench-npm-file-url');expected='b8b02f99245ee4e3392c84b75480733d7f135802'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==expected
source=Path('/workspace/quench/target/npm-file-url/iteration/quench-node');target=Path('/workspace/scratch/npm-fs-sync-candidate');shutil.copy2(source,target)
node=Path(shutil.which('node'))
Path('/workspace/scratch/npm-fs-sync-engines.json').write_text(json.dumps({'candidate':{'source':expected,'sha256':hashlib.sha256(target.read_bytes()).hexdigest()},'baseline':{'source':'faa120035a332d3b5eb232eaadcad1399f306d7d','published_merge':'1f36ad64d05505dc438d381cb2d1dd9740f96140','sha256':hashlib.sha256(Path('/workspace/scratch/npm-canonical-sync-candidate').read_bytes()).hexdigest()},'node':{'path':str(node),'version':subprocess.check_output([node,'--version'],text=True).strip(),'sha256':hashlib.sha256(node.read_bytes()).hexdigest()}},indent=2)+'\n')
PY
export QUENCH_NODE_TEST_EXECUTABLE=/workspace/scratch/npm-fs-sync-candidate
export QUENCH_NODE_LIBRARY_ROOT=/workspace/scratch/quench-npm-file-url/tests/node
run node-api cargo test -p quench-node-test --test child_entry -- --test-threads=1
run node-lib cargo test -p quench-node --lib
run opt-core cargo test --profile iteration -p quench-runtime --lib
