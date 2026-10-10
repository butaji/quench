#!/usr/bin/env python3
import hashlib, importlib.util, json, os, subprocess
from pathlib import Path

ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley'
spec = importlib.util.spec_from_file_location(
    'screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(engine['environment'] for engine in raw['engines'] if engine['name'] == 'quench')
base = OUT / 'candidate-binary'
cand = ROOT / 'target/production/quench-node'
fixture = mod.materialize('earley-boyer')

probe = OUT / 'getfield-local-node-oracle.js'
node = '/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
oracle = subprocess.run([node, str(probe)], env=env, text=True, capture_output=True, check=True)
oracle_semantic = [line for line in oracle.stdout.splitlines() if line]
baseline_oracle = mod.run(base.resolve(), probe, env, 'getfield-local-node-baseline')
candidate_oracle = mod.run(cand.resolve(), probe, env, 'getfield-local-node-candidate')
if not (baseline_oracle['exit_code'] == candidate_oracle['exit_code'] == 0
        and baseline_oracle['semantic_output'] == candidate_oracle['semantic_output']
        and candidate_oracle['semantic_output'] == oracle_semantic):
    raise RuntimeError(json.dumps({
        'node': oracle_semantic,
        'baseline': baseline_oracle,
        'candidate': candidate_oracle,
    }, indent=2))
results = []
for pair in range(1, 4):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    pair_results = {}
    for variant in order:
        binary = base if variant == 'baseline' else cand
        pair_results[variant] = mod.run(binary.resolve(), fixture, env,
                                        f'getfield-local-{pair:02d}-{variant}')
    b, c = pair_results['baseline'], pair_results['candidate']
    result = {
        'pair': pair,
        'order': order,
        'baseline_score': b['score'],
        'candidate_score': c['score'],
        'score_delta': c['score'] - b['score'],
        'baseline_rss_bytes': b['maximum_rss_bytes'],
        'candidate_rss_bytes': c['maximum_rss_bytes'],
        'rss_delta_bytes': c['maximum_rss_bytes'] - b['maximum_rss_bytes'],
        'fixture_output_equal': b['semantic_output'] == c['semantic_output'],
        'baseline_exit': b['exit_code'],
        'candidate_exit': c['exit_code'],
        'valid': b['exit_code'] == c['exit_code'] == 0
                 and b['score'] is not None and c['score'] is not None
                 and b['semantic_output'] == c['semantic_output'],
    }
    results.append(result)
    print(f"pair {pair}/3: Score {b['score']}->{c['score']} ({result['score_delta']:+g}); "
          f"RSS {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} "
          f"({result['rss_delta_bytes']:+d}); output_equal={result['fixture_output_equal']}",
          flush=True)

report = {
    'experiment': 'fuse LoadLocalPlain -> direct Atom GetField on EarleyBoyer',
    'candidate_sha256': hashlib.sha256(cand.read_bytes()).hexdigest(),
    'baseline_sha256': hashlib.sha256(base.read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(),
    'node_oracle_sha256': hashlib.sha256(probe.read_bytes()).hexdigest(),
    'node_oracle_output': oracle_semantic,
    'node_oracle_match': True,
    'pairs': results,
}
(OUT / 'getfield-local-screen-3.json').write_text(json.dumps(report, indent=2) + '\n')
