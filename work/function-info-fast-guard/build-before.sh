#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
cargo check --profile iteration -p quench-runtime -p quench-node --lib -j1 > work/function-info-fast-guard/before-check.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/function-info-fast-guard/before-build.log 2>&1
python3 - <<'FREEZE'
from pathlib import Path
import hashlib,json,shutil,subprocess
b=Path('work/function-info-before-bin');assert not b.exists();b.mkdir()
for n in ['quench-node','run-parallel']:shutil.copy2(Path('target/iteration')/n,b/n)
shutil.copy2('work/sdk-primitives-bin/openssl-cli',b/'openssl-cli')
files=subprocess.check_output(['git','ls-files','crates'],text=True).splitlines()
(b/'source-sha256.json').write_text(json.dumps({n:hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in files if Path(n).is_file()},indent=2)+'\n')
(b/'source-commit.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
(b/'sha256.txt').write_text(''.join(hashlib.sha256((b/n).read_bytes()).hexdigest()+'  '+n+'\n'for n in ['quench-node','run-parallel','openssl-cli']))
FREEZE
