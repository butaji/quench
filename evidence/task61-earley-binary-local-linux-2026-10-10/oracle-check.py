#!/usr/bin/env python3
import importlib.util
import json
import subprocess
from pathlib import Path

ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-earley-binary-local-linux-2026-10-10'
spec = importlib.util.spec_from_file_location(
    'screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(engine['environment'] for engine in raw['engines'] if engine['name'] == 'quench')
node = '/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
probe = OUT / 'node-oracle.js'
expected = subprocess.run([node, str(probe)], env=env, text=True,
                          capture_output=True, check=True)
expected_lines = [line for line in expected.stdout.splitlines() if line]
for name, binary in [
    ('baseline', OUT / 'move-move-baseline'),
    ('candidate', OUT / 'binary-local-candidate'),
]:
    actual = subprocess.run([str(binary), str(probe)], env=env, text=True,
                            capture_output=True, check=True)
    actual_lines = [line for line in actual.stdout.splitlines() if line]
    if actual_lines != expected_lines:
        raise SystemExit(f'{name} differs from Node: {actual_lines!r} != {expected_lines!r}')
    print(f'{name} matches Node: {actual_lines[0]}')
