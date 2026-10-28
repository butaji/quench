#!/usr/bin/env python3
import hashlib, importlib.util, json, random, statistics, subprocess
from pathlib import Path

ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley'
BOOT = 100000
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

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def interval(values, seed):
    rng = random.Random(seed)
    draws = sorted(statistics.median(rng.choices(values, k=len(values)))
                   for _ in range(BOOT))
    return [draws[int(.025 * BOOT)], draws[int(.975 * BOOT)]]

prior = json.loads((OUT / 'move-move-paired-21.json').read_text())
assert prior['baseline_sha256'] == sha(base)
assert prior['candidate_sha256'] == sha(cand)
assert prior['fixture_sha256'] == sha(fixture)
expected = subprocess.run([node, str(probe)], env=env, text=True,
                          capture_output=True, check=True)
oracle_output = [line for line in expected.stdout.splitlines() if line]
for name, binary in [('baseline', base), ('candidate', cand)]:
    result = subprocess.run([str(binary), str(probe)], env=env, text=True,
                            capture_output=True, check=True)
    if [line for line in result.stdout.splitlines() if line] != oracle_output:
        raise RuntimeError(f'{name} does not match Node oracle')

pairs = list(prior['pairs'])
log = OUT / 'move-move-pairs-32.jsonl'
log.write_text(''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in pairs))
for pair in range(22, 33):
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
    with log.open('a') as output:
        output.write(json.dumps(row, separators=(',', ':')) + '\n')
        output.flush()
    print(f"pair {pair}/32: Score {b['score']}->{c['score']} "
          f"({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->"
          f"{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); equal={equal}",
          flush=True)

scores = [row['score_delta'] for row in pairs]
rss = [row['rss_delta_bytes'] for row in pairs]
report = {
    'schema': 1,
    'experiment': 'fuse Move -> Move on EarleyBoyer, on top of CopyLocalPlain',
    'baseline_sha256': sha(base),
    'candidate_sha256': sha(cand),
    'fixture_sha256': sha(fixture),
    'node_oracle_sha256': sha(probe),
    'node_oracle_output': oracle_output,
    'rounds': len(pairs),
    'bootstrap_replicates': BOOT,
    'baseline_score_median': statistics.median(row['baseline_score'] for row in pairs),
    'candidate_score_median': statistics.median(row['candidate_score'] for row in pairs),
    'score_delta_median': statistics.median(scores),
    'score_delta_95_ci': interval(scores, 0x610eca3),
    'baseline_maximum_rss_median_bytes': statistics.median(
        row['baseline_rss_bytes'] for row in pairs),
    'candidate_maximum_rss_median_bytes': statistics.median(
        row['candidate_rss_bytes'] for row in pairs),
    'maximum_rss_delta_median_bytes': statistics.median(rss),
    'maximum_rss_delta_95_ci_bytes': interval(rss, 0x610eca4),
    'candidate_score_worse_pairs': sum(delta < 0 for delta in scores),
    'candidate_rss_lower_pairs': sum(delta < 0 for delta in rss),
    'valid': all(row['valid'] for row in pairs),
    'output_equal': all(row['output_equal'] for row in pairs),
    'qualification_ready': False,
    'pairs': pairs,
}
(OUT / 'move-move-paired-32.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({key: value for key, value in report.items() if key != 'pairs'}, indent=2))
