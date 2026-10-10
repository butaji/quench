#!/usr/bin/env python3
import hashlib, importlib.util, json, subprocess
from pathlib import Path

ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley'
spec = importlib.util.spec_from_file_location(
    'screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(engine['environment'] for engine in raw['engines'] if engine['name'] == 'quench')
base = (OUT / 'copy-local-candidate').resolve()
cand = (ROOT / 'target/production/quench-node').resolve()
fixture = mod.materialize('earley-boyer')
probe = OUT / 'move-move-node-oracle.js'
node = '/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
oracle = subprocess.run([node, str(probe)], env=env, text=True,
                        capture_output=True, check=True)
oracle_output = [line for line in oracle.stdout.splitlines() if line]
for name, binary in [('baseline', base), ('candidate', cand)]:
    result = subprocess.run([str(binary), str(probe)], env=env, text=True,
                            capture_output=True, check=True)
    output = [line for line in result.stdout.splitlines() if line]
    if output != oracle_output:
        raise RuntimeError(f'{name} differs from Node: {output!r} != {oracle_output!r}')

pairs = []
for pair in range(1, 4):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        results[variant] = mod.run(base if variant == 'baseline' else cand,
                                   fixture, env, f'move-move-{pair:02d}-{variant}')
    b, c = results['baseline'], results['candidate']
    equal = b['semantic_output'] == c['semantic_output']
    row = {
        'pair': pair,
        'order': order,
        'valid': b['exit_code'] == c['exit_code'] == 0
                 and b['score'] is not None and c['score'] is not None and equal,
        'output_equal': equal,
        'baseline_score': b['score'],
        'candidate_score': c['score'],
        'score_delta': c['score'] - b['score'],
        'baseline_rss_bytes': b['maximum_rss_bytes'],
        'candidate_rss_bytes': c['maximum_rss_bytes'],
        'rss_delta_bytes': c['maximum_rss_bytes'] - b['maximum_rss_bytes'],
        'baseline_wall_ns': b['wall_ns'],
        'candidate_wall_ns': c['wall_ns'],
    }
    pairs.append(row)
    print(f"pair {pair}/3: Score {b['score']}->{c['score']} "
          f"({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->"
          f"{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); equal={equal}",
          flush=True)

report = {
    'experiment': 'fuse Move -> Move on EarleyBoyer, on top of CopyLocalPlain',
    'baseline_sha256': hashlib.sha256(base.read_bytes()).hexdigest(),
    'candidate_sha256': hashlib.sha256(cand.read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(),
    'node_oracle_sha256': hashlib.sha256(probe.read_bytes()).hexdigest(),
    'node_oracle_output': oracle_output,
    'node_oracle_match': True,
    'pairs': pairs,
}
(OUT / 'move-move-screen-3.json').write_text(json.dumps(report, indent=2) + '\n')
