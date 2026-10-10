#!/usr/bin/env python3
import hashlib, importlib.util, json, statistics, subprocess
from pathlib import Path
ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley/get-field-store-local'
spec = importlib.util.spec_from_file_location('screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(item['environment'] for item in raw['engines'] if item['name'] == 'quench')
base = (ROOT / 'work/task61-v2-merge-earley/initialized-this-baseline/quench-node').resolve()
candidate = (ROOT / 'work/task61-v2-merge-earley/candidate-target/production/quench-node').resolve()
fixture = runner.materialize('earley-boyer')
probe = OUT / 'node-oracle.cjs'
node = Path('/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node')
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
expected = subprocess.run([str(node), str(probe)], env=env, text=True, capture_output=True, check=True).stdout.splitlines()
for label, binary in [('baseline', base), ('candidate', candidate)]:
    result = subprocess.run([str(binary), str(probe)], env=env, text=True, capture_output=True, check=True)
    if result.stdout.splitlines() != expected:
        raise RuntimeError(f'{label} does not match Node oracle: {result.stdout!r} != {expected!r}')
prior_path = OUT / 'screen-3.jsonl'
prior = [json.loads(line) for line in prior_path.read_text().splitlines()] if prior_path.exists() else []
pairs = list(prior)
for pair in range(len(pairs) + 1, 4):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        results[variant] = runner.run(base if variant == 'baseline' else candidate, fixture, env,
                                      f'earley-field-store-{pair:02d}-{variant}')
    b, c = results['baseline'], results['candidate']
    row = {
        'pair': pair, 'order': order,
        'valid': b['exit_code'] == c['exit_code'] == 0 and b['score'] is not None and c['score'] is not None and b['semantic_output'] == c['semantic_output'],
        'output_equal': b['semantic_output'] == c['semantic_output'],
        'baseline_score': b['score'], 'candidate_score': c['score'], 'score_delta': c['score'] - b['score'],
        'baseline_rss_bytes': b['maximum_rss_bytes'], 'candidate_rss_bytes': c['maximum_rss_bytes'],
        'rss_delta_bytes': c['maximum_rss_bytes'] - b['maximum_rss_bytes'],
        'baseline_wall_ns': b['wall_ns'], 'candidate_wall_ns': c['wall_ns'],
        'baseline_output': b['semantic_output'], 'candidate_output': c['semantic_output'],
    }
    pairs.append(row)
    with (OUT / 'screen-3.jsonl').open('a') as f:
        f.write(json.dumps(row, separators=(',', ':')) + '\n'); f.flush()
    print(f"pair {pair}/3: Score {b['score']}->{c['score']} ({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); output_equal={row['output_equal']}", flush=True)
report = {
    'schema': 1,
    'experiment': 'fuse GetField -> StoreLocalPlain on EarleyBoyer',
    'source_revision': subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
    'baseline_sha256': sha(base), 'candidate_sha256': sha(candidate), 'fixture_sha256': sha(fixture), 'node_oracle_sha256': sha(probe),
    'rounds': len(pairs),
    'baseline_score_median': statistics.median(x['baseline_score'] for x in pairs),
    'candidate_score_median': statistics.median(x['candidate_score'] for x in pairs),
    'score_delta_median': statistics.median(x['score_delta'] for x in pairs),
    'baseline_maximum_rss_median_bytes': statistics.median(x['baseline_rss_bytes'] for x in pairs),
    'candidate_maximum_rss_median_bytes': statistics.median(x['candidate_rss_bytes'] for x in pairs),
    'maximum_rss_delta_median_bytes': statistics.median(x['rss_delta_bytes'] for x in pairs),
    'valid': all(x['valid'] for x in pairs), 'output_equal': all(x['output_equal'] for x in pairs),
    'pairs': pairs,
}
(OUT / 'screen-3.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k:v for k,v in report.items() if k != 'pairs'}, indent=2))
