#!/usr/bin/env python3
import hashlib
import importlib.util
import json
from pathlib import Path

ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-earley-binary-local-linux-2026-10-10'
spec = importlib.util.spec_from_file_location(
    'screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(engine['environment'] for engine in raw['engines'] if engine['name'] == 'quench')
base = OUT / 'move-move-baseline'
cand = ROOT / 'target/production/quench-node'
fixture = mod.materialize('earley-boyer')

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

pairs = []
for pair in range(1, 4):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        results[variant] = mod.run(base if variant == 'baseline' else cand,
                                   fixture, env,
                                   f'binary-local-{pair:02d}-{variant}')
    b, c = results['baseline'], results['candidate']
    row = {
        'pair': pair,
        'order': order,
        'valid': b['exit_code'] == c['exit_code'] == 0
                 and b['score'] is not None and c['score'] is not None
                 and b['semantic_output'] == c['semantic_output'],
        'output_equal': b['semantic_output'] == c['semantic_output'],
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
          f"{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); "
          f"equal={row['output_equal']}", flush=True)

report = {
    'experiment': 'fuse LoadLocalPlain -> Binary using a validated direct local read',
    'baseline_sha256': sha(base),
    'candidate_sha256': sha(cand),
    'fixture_sha256': sha(fixture),
    'pairs': pairs,
    'valid': all(row['valid'] for row in pairs),
}
(OUT / 'screen-3.json').write_text(json.dumps(report, indent=2) + '\n')
