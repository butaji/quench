import hashlib
import importlib.util
import json
import pathlib
import statistics

ROOT = pathlib.Path('/workspace/quench')
OUT = ROOT / 'work/task61-v2-merge-earley'
spec = importlib.util.spec_from_file_location(
    'screen_runner', ROOT / 'work/task61-direct-number-screen/runner.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

raw = json.loads(
    (ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json')
    .read_text())
env = next(engine['environment'] for engine in raw['engines']
           if engine['name'] == 'quench')
baseline = OUT / 'loadconst-capture-binaries/baseline-binary'
candidate = OUT / 'loadcapture-local-late-candidate'
fixture = runner.materialize('earley-boyer')
oracle = OUT / 'loadconst-capture-node-oracle.js'
node = '/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
log = OUT / 'loadcapture-local-late-pairs-3.jsonl'
report_path = OUT / 'loadcapture-local-late-screen-3.json'
if log.exists() or report_path.exists():
    raise SystemExit('refusing to overwrite existing late-fusion screen evidence')

node_output = runner.run(node, oracle, env, 'loadcapture-local-late-node-oracle')
baseline_oracle = runner.run(baseline.resolve(), oracle, env,
                             'loadcapture-local-late-baseline-oracle')
candidate_oracle = runner.run(candidate.resolve(), oracle, env,
                              'loadcapture-local-late-candidate-oracle')
if not (node_output['exit_code'] == baseline_oracle['exit_code']
        == candidate_oracle['exit_code'] == 0
        and node_output['semantic_output'] == baseline_oracle['semantic_output']
        == candidate_oracle['semantic_output']):
    raise RuntimeError('Node, baseline, and candidate oracle output differs')

pairs = []
for pair in range(1, 4):
    order = ['baseline', 'candidate'] if pair % 2 else ['candidate', 'baseline']
    results = {}
    for variant in order:
        binary = baseline if variant == 'baseline' else candidate
        results[variant] = runner.run(
            binary.resolve(), fixture, env,
            f'loadcapture-local-late-paired-{pair:02d}-{variant}')
    before, after = results['baseline'], results['candidate']
    equal = before['semantic_output'] == after['semantic_output']
    row = {
        'pair': pair,
        'order': order,
        'baseline_score': before['score'],
        'candidate_score': after['score'],
        'score_delta': after['score'] - before['score'],
        'baseline_rss_bytes': before['maximum_rss_bytes'],
        'candidate_rss_bytes': after['maximum_rss_bytes'],
        'rss_delta_bytes': after['maximum_rss_bytes'] - before['maximum_rss_bytes'],
        'output_equal': equal,
        'baseline_exit': before['exit_code'],
        'candidate_exit': after['exit_code'],
        'valid': before['exit_code'] == after['exit_code'] == 0
                 and before['score'] is not None and after['score'] is not None
                 and equal,
    }
    pairs.append(row)
    with log.open('a') as handle:
        handle.write(json.dumps(row, separators=(',', ':')) + '\n')
        handle.flush()
    print(f"pair {pair}/3: Score {before['score']}->{after['score']} "
          f"({row['score_delta']:+g}); RSS "
          f"{before['maximum_rss_bytes']}->{after['maximum_rss_bytes']} "
          f"({row['rss_delta_bytes']:+d}); equal={equal}", flush=True)

report = {
    'experiment': 'EarleyBoyer late LoadCapture -> LoadLocalPlain fusion',
    'baseline_sha256': hashlib.sha256(baseline.read_bytes()).hexdigest(),
    'candidate_sha256': hashlib.sha256(candidate.read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(),
    'node_oracle_sha256': hashlib.sha256(oracle.read_bytes()).hexdigest(),
    'node_oracle_match': True,
    'pairs': pairs,
    'baseline_score_median': statistics.median(p['baseline_score'] for p in pairs),
    'candidate_score_median': statistics.median(p['candidate_score'] for p in pairs),
    'score_delta_median': statistics.median(p['score_delta'] for p in pairs),
    'baseline_maximum_rss_median_bytes': statistics.median(
        p['baseline_rss_bytes'] for p in pairs),
    'candidate_maximum_rss_median_bytes': statistics.median(
        p['candidate_rss_bytes'] for p in pairs),
    'maximum_rss_delta_median_bytes': statistics.median(
        p['rss_delta_bytes'] for p in pairs),
    'valid': all(p['valid'] for p in pairs),
    'output_equal': all(p['output_equal'] for p in pairs),
    'qualification_ready': False,
}
report_path.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({key: value for key, value in report.items() if key != 'pairs'},
                 indent=2))
