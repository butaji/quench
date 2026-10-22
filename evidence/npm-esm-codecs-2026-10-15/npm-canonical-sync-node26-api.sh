#!/usr/bin/env bash
set -u
source /workspace/quench-build-env.sh
export PATH="/workspace/shared/node-v26.12.0-linux-x64/bin:$PATH"
export QUENCH_NODE_TEST_EXECUTABLE=/workspace/scratch/npm-canonical-sync-candidate
export QUENCH_NODE_LIBRARY_ROOT=/workspace/scratch/quench-npm-file-url/tests/node
cd /workspace/scratch/quench-npm-file-url
python3 - <<'PY'
import subprocess,shutil,hashlib,json
from pathlib import Path
node=Path(shutil.which('node'));cli=Path('/workspace/scratch/npm-canonical-sync-candidate');test=Path('/workspace/quench/target/npm-file-url/debug/deps/child_entry-94fd2328f8a45425')
Path('/workspace/scratch/npm-canonical-sync-node-api-node26-provenance.json').write_text(json.dumps({'oracle_path':str(node),'oracle_version':subprocess.check_output([node,'--version'],text=True).strip(),'oracle_sha256':hashlib.sha256(node.read_bytes()).hexdigest(),'candidate_source':'faa120035a332d3b5eb232eaadcad1399f306d7d','candidate_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),'fixture_source':'0cc26891e137f6ac14bb36b5c657a27303599b67','test_binary':str(test),'test_sha256':hashlib.sha256(test.read_bytes()).hexdigest()},indent=2)+'\n')
PY
/workspace/quench/target/npm-file-url/debug/deps/child_entry-94fd2328f8a45425 --test-threads=1 > /workspace/scratch/npm-canonical-sync-node-api-node26.log 2>&1
status=$?
printf '%s\n' "$status" > /workspace/scratch/npm-canonical-sync-node-api-node26.exit
