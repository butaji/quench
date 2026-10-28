#!/bin/bash
set -euo pipefail
cd /workspace/quench
python3 - <<'WAIT'
import json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/buffer-full-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
bash work/native-buffer-unsafe/qualify.sh
