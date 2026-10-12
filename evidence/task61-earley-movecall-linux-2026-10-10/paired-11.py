#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import random
import statistics
import subprocess
from pathlib import Path
ROOT = Path('/workspace/quench')
OUT = ROOT / 'work/task61-earley-movecall-linux-2026-10-10'
BOOT = 100000
spec = importlib.util.spec_from_file_location('benchrunner', ROOT / 'work/task61-direct-number-screen/runner.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
raw = json.loads((ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env = next(e['environment'] for e in raw['engines'] if e['name'] == 'quench')
base = OUT / 'baseline'
candidate = OUT / 'candidate'
fixture = mod.materialize('earley-boyer')
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def interval(values, seed):
    rng = random.Random(seed)
    draws = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(BOOT))
    return [draws[int(.025 * BOOT)], draws[int(.975 * BOOT)]]
pilot = json.loads((OUT / 'screen-3.json').read_text())
assert pilot['baseline_sha256'] == sha(base)
assert pilot['candidate_sha256'] == sha(candidate)
assert pilot['fixture_sha256'] == sha(fixture)
pairs = list(pilot['pairs'])
log = OUT / 'pairs-11.jsonl'
log.write_text(''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in pairs))
for pair in range(4, 12):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        results[variant] = mod.run(base if variant == 'baseline' else candidate, fixture, env,
                                   f'movecall-pair-{pair:02d}-{variant}')
    b, c = results['baseline'], results['candidate']
    equal = b['semantic_output'] == c['semantic_output']
    row = {
        'pair': pair, 'order': order,
        'valid': b['exit_code'] == c['exit_code'] == 0 and b['score'] is not None and c['score'] is not None and equal,
        'output_equal': equal,
        'baseline_score': b['score'], 'candidate_score': c['score'], 'score_delta': c['score'] - b['score'],
        'baseline_rss_bytes': b['maximum_rss_bytes'], 'candidate_rss_bytes': c['maximum_rss_bytes'],
        'rss_delta_bytes': c['maximum_rss_bytes'] - b['maximum_rss_bytes'],
        'baseline_wall_ns': b['wall_ns'], 'candidate_wall_ns': c['wall_ns'],
    }
    pairs.append(row)
    with log.open('a') as f:
        f.write(json.dumps(row, separators=(',', ':')) + '\n'); f.flush()
    print(f"pair {pair}/11: Score {b['score']}->{c['score']} ({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); output_equal={equal}", flush=True)
scores = [x['score_delta'] for x in pairs]
rss = [x['rss_delta_bytes'] for x in pairs]
report = {
    'schema': 1,
    'experiment': 'forward one-argument Move -> Call window source on EarleyBoyer',
    'baseline_sha256': sha(base), 'candidate_sha256': sha(candidate), 'fixture_sha256': sha(fixture),
    'source_revision': subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
    'rounds': len(pairs), 'bootstrap_replicates': BOOT,
    'baseline_score_median': statistics.median(x['baseline_score'] for x in pairs),
    'candidate_score_median': statistics.median(x['candidate_score'] for x in pairs),
    'score_delta_median': statistics.median(scores), 'score_delta_95_ci': interval(scores, 0x610eca3),
    'baseline_maximum_rss_median_bytes': statistics.median(x['baseline_rss_bytes'] for x in pairs),
    'candidate_maximum_rss_median_bytes': statistics.median(x['candidate_rss_bytes'] for x in pairs),
    'maximum_rss_delta_median_bytes': statistics.median(rss), 'maximum_rss_delta_95_ci_bytes': interval(rss, 0x610eca4),
    'candidate_score_worse_pairs': sum(x < 0 for x in scores), 'candidate_rss_lower_pairs': sum(x < 0 for x in rss),
    'valid': all(x['valid'] for x in pairs), 'output_equal': all(x['output_equal'] for x in pairs),
    'qualification_ready': False, 'pairs': pairs,
}
(OUT / 'paired-11.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k:v for k,v in report.items() if k!='pairs'}, indent=2))
