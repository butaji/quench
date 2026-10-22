#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import statistics
import subprocess
from pathlib import Path

ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley'
spec = importlib.util.spec_from_file_location(
    'screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(engine['environment'] for engine in raw['engines'] if engine['name'] == 'quench')
base = (ROOT / 'target/production/quench-node').resolve()
cand = (OUT / 'candidate-target/production/quench-node').resolve()
fixture = mod.materialize('earley-boyer')
node = Path('/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node')

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def semantic(result):
    return result['semantic_output']

node_result = mod.run(node, fixture, env, 'set-this-earley-node')
if node_result['exit_code'] != 0 or node_result['score'] is None:
    raise RuntimeError(f'Node oracle failed: {node_result}')
node_semantic = semantic(node_result)
pairs = []
for pair in range(1, 4):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        results[variant] = mod.run(base if variant == 'baseline' else cand,
                                   fixture, env, f'local-setter-screen-{pair:02d}-{variant}')
    b, c = results['baseline'], results['candidate']
    row = {
        'pair': pair,
        'order': order,
        'valid': b['exit_code'] == c['exit_code'] == 0
                 and b['score'] is not None and c['score'] is not None
                 and semantic(b) == semantic(c) == node_semantic,
        'node_equal': semantic(b) == semantic(c) == node_semantic,
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
    with (OUT / 'local-setter-screen-3.jsonl').open('a') as log:
        log.write(json.dumps(row, separators=(',', ':')) + '\n')
        log.flush()
    print(f"pair {pair}/3: Score {b['score']}->{c['score']} "
          f"({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->"
          f"{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); "
          f"Node-equal={row['node_equal']}", flush=True)

report = {
    'experiment': 'fuse LoadLocalPlain -> SetThisFieldStrict on EarleyBoyer',
    'baseline_sha256': sha(base),
    'candidate_sha256': sha(cand),
    'fixture_sha256': sha(fixture),
    'node_score': node_result['score'],
    'node_semantic_output': node_semantic,
    'rounds': len(pairs),
    'baseline_score_median': statistics.median(row['baseline_score'] for row in pairs),
    'candidate_score_median': statistics.median(row['candidate_score'] for row in pairs),
    'score_delta_median': statistics.median(row['score_delta'] for row in pairs),
    'baseline_maximum_rss_median_bytes': statistics.median(row['baseline_rss_bytes'] for row in pairs),
    'candidate_maximum_rss_median_bytes': statistics.median(row['candidate_rss_bytes'] for row in pairs),
    'maximum_rss_delta_median_bytes': statistics.median(row['rss_delta_bytes'] for row in pairs),
    'valid': all(row['valid'] for row in pairs),
    'pairs': pairs,
}
(OUT / 'local-setter-screen-3.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({key: value for key, value in report.items() if key != 'pairs'}, indent=2))
