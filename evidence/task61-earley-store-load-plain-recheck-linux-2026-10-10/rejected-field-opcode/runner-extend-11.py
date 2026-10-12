#!/usr/bin/env python3
import hashlib, importlib.util, json, random, statistics, subprocess
from pathlib import Path
ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley/get-field-store-local'
spec = importlib.util.spec_from_file_location('screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
runner = importlib.util.module_from_spec(spec); spec.loader.exec_module(runner)
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
    if result.stdout.splitlines() != expected: raise RuntimeError(f'{label} differs from Node oracle')
log = OUT / 'paired-11.jsonl'
pairs = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else [json.loads(line) for line in (OUT / 'screen-3.jsonl').read_text().splitlines()]
assert len(pairs) == 3
for pair in range(4, 12):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        results[variant] = runner.run(base if variant == 'baseline' else candidate, fixture, env, f'earley-field-store-{pair:02d}-{variant}')
    b, c = results['baseline'], results['candidate']
    row = {
        'pair': pair, 'order': order,
        'valid': b['exit_code'] == c['exit_code'] == 0 and b['score'] is not None and c['score'] is not None and b['semantic_output'] == c['semantic_output'],
        'output_equal': b['semantic_output'] == c['semantic_output'],
        'baseline_score': b['score'], 'candidate_score': c['score'], 'score_delta': c['score'] - b['score'],
        'baseline_rss_bytes': b['maximum_rss_bytes'], 'candidate_rss_bytes': c['maximum_rss_bytes'], 'rss_delta_bytes': c['maximum_rss_bytes'] - b['maximum_rss_bytes'],
        'baseline_wall_ns': b['wall_ns'], 'candidate_wall_ns': c['wall_ns'],
        'baseline_output': b['semantic_output'], 'candidate_output': c['semantic_output'],
    }
    pairs.append(row)
    with log.open('a') as f: f.write(json.dumps(row, separators=(',', ':')) + '\n'); f.flush()
    print(f"pair {pair}/11: Score {b['score']}->{c['score']} ({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); equal={row['output_equal']}", flush=True)

def interval(values, seed, draws=100_000):
    rng = random.Random(seed)
    samples = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(draws))
    return [samples[int(.025 * draws)], samples[int(.975 * draws)]]
scores = [r['score_delta'] for r in pairs]
rss = [r['rss_delta_bytes'] for r in pairs]
report = {
    'schema': 1, 'experiment': 'fuse GetField -> StoreLocalPlain on EarleyBoyer',
    'source_revision': subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
    'baseline_sha256': sha(base), 'candidate_sha256': sha(candidate), 'fixture_sha256': sha(fixture), 'node_oracle_sha256': sha(probe),
    'rounds': len(pairs), 'bootstrap_replicates': 100_000,
    'baseline_score_median': statistics.median(r['baseline_score'] for r in pairs),
    'candidate_score_median': statistics.median(r['candidate_score'] for r in pairs),
    'score_delta_median': statistics.median(scores), 'score_delta_95_ci': interval(scores, 0x610eca9),
    'baseline_maximum_rss_median_bytes': statistics.median(r['baseline_rss_bytes'] for r in pairs),
    'candidate_maximum_rss_median_bytes': statistics.median(r['candidate_rss_bytes'] for r in pairs),
    'maximum_rss_delta_median_bytes': statistics.median(rss), 'maximum_rss_delta_95_ci_bytes': interval(rss, 0x610ecaa),
    'candidate_score_worse_pairs': sum(x < 0 for x in scores), 'candidate_rss_lower_pairs': sum(x < 0 for x in rss),
    'valid': all(r['valid'] for r in pairs), 'output_equal': all(r['output_equal'] for r in pairs), 'qualification_ready': False, 'pairs': pairs,
}
(OUT / 'paired-11.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k:v for k,v in report.items() if k != 'pairs'}, indent=2))
