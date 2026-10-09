import hashlib
import json
import os
import pathlib
import random
import re
import statistics
import subprocess
import time

root = pathlib.Path('/workspace/quench')
work = root / 'target/stageb-candidates/array-index-lookup'
suite = root / 'quench-bench/js-engine-benchmark/v8-v7'
binaries = {
    'baseline': work / 'baseline-quench-node',
    'candidate': work / 'candidate-quench-node',
}
node = pathlib.Path('/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node')
env = {key: value for key, value in os.environ.items() if key in {'PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'TZ'}}
main_rs = (root / 'quench-bench/src/main.rs').read_text()
runner = re.search(r'const RUNNER: &str = r#"(.*?)"#;', main_rs, re.S).group(1)
fixtures = ['navier-stokes']

def sha_bytes(data):
    return hashlib.sha256(data).hexdigest()
def sha_file(path):
    return sha_bytes(path.read_bytes())
def semantic(text):
    return '\n'.join(line for line in text.splitlines() if not line.startswith(('Score: ', '__quenchBenchResult: ')) and line != '----')
def bootstrap(values, seed):
    rng = random.Random(seed)
    samples = sorted(statistics.median(values[rng.randrange(len(values))] for _ in values) for _ in range(100000))
    return [samples[2499], samples[97499]]
def run(label, fixture_path, ordinal, name):
    stdout_path = work / f'{name}-{ordinal:02d}-{label}.stdout'
    stderr_path = work / f'{name}-{ordinal:02d}-{label}.stderr'
    started = time.perf_counter_ns()
    pid = os.fork()
    if pid == 0:
        with stdout_path.open('wb') as stdout, stderr_path.open('wb') as stderr:
            os.dup2(stdout.fileno(), 1)
            os.dup2(stderr.fileno(), 2)
            os.execve(str(binaries[label]), [str(binaries[label]), str(fixture_path)], env)
    waited, status, usage = os.wait4(pid, 0)
    assert waited == pid
    text = stdout_path.read_text(errors='replace')
    scores = [line.removeprefix('Score: ') for line in text.splitlines() if line.startswith('Score: ')]
    return {'status': os.waitstatus_to_exitcode(status), 'elapsed_ns': time.perf_counter_ns()-started,
            'score': float(scores[-1]) if len(scores) == 1 else None, 'max_rss_bytes': usage.ru_maxrss*1024,
            'stdout_sha256': sha_file(stdout_path), 'stderr_sha256': sha_file(stderr_path),
            'semantic_sha256': sha_bytes(semantic(text).encode()), 'semantic': semantic(text)}

def summarize(values, seed):
    return {'median': statistics.median(values), '95_ci': bootstrap(values, seed)}

all_reports = {}
source_revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
source_diff = subprocess.check_output(['git', 'diff', '--binary'], cwd=root)
source_diff_sha256 = sha_bytes(source_diff)
host = {'platform': subprocess.check_output(['uname', '-a'], text=True).strip(), 'cpu_quota': pathlib.Path('/sys/fs/cgroup/cpu.max').read_text().strip() if pathlib.Path('/sys/fs/cgroup/cpu.max').exists() else None, 'memory_limit_bytes': pathlib.Path('/sys/fs/cgroup/memory.max').read_text().strip() if pathlib.Path('/sys/fs/cgroup/memory.max').exists() else None}
for fixture in fixtures:
    name = fixture.replace('-', '_')
    src = suite / f'{fixture}.js'
    fixture_data = (suite / 'base.js').read_bytes() + b'\n' + src.read_bytes() + b'\n' + runner.encode()
    materialized = work / f'{name}-materialized.js'
    materialized.write_bytes(fixture_data)
    rows = []
    for pair in range(3):
        order = ('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')
        samples = {label: run(label, materialized, pair*2+idx, name) for idx, label in enumerate(order)}
        assert all(sample['status'] == 0 and sample['score'] is not None for sample in samples.values()), (fixture, pair, samples)
        assert samples['baseline']['semantic'] == samples['candidate']['semantic'], (fixture, pair)
        rows.append({'pair': pair+1, 'order': order, **{k: {key: val for key, val in v.items() if key != 'semantic'} for k, v in samples.items()}})
        checkpoint = work / f'{name}-paired-progress.json'
        checkpoint.write_text(json.dumps({'fixture': fixture, 'rows': rows}, indent=2)+'\n')
        delta = samples['candidate']['max_rss_bytes']-samples['baseline']['max_rss_bytes']
        print(f'{fixture}: pair {pair+1}/3 baseline={samples["baseline"]["score"]} candidate={samples["candidate"]["score"]} rss_delta={delta}', flush=True)
    base_scores = [r['baseline']['score'] for r in rows]
    cand_scores = [r['candidate']['score'] for r in rows]
    score_delta = [c-b for b,c in zip(base_scores,cand_scores)]
    base_rss = [r['baseline']['max_rss_bytes'] for r in rows]
    cand_rss = [r['candidate']['max_rss_bytes'] for r in rows]
    rss_delta = [c-b for b,c in zip(base_rss,cand_rss)]
    report = {'schema': 1, 'fixture': fixture, 'source_revision': source_revision, 'source_dirty': True, 'source_diff_sha256': source_diff_sha256, 'host': host, 'bootstrap': {'method': 'paired percentile bootstrap of median differences', 'replicates': 100000}, 'fixture_sha256': sha_bytes(fixture_data),
              'baseline_binary_sha256': sha_file(binaries['baseline']), 'candidate_binary_sha256': sha_file(binaries['candidate']),
              'pairs': rows, 'summary': {'baseline_score_median': statistics.median(base_scores),
              'candidate_score_median': statistics.median(cand_scores), 'score_delta': summarize(score_delta, 6101101+fixtures.index(fixture)),
              'baseline_rss_median_bytes': statistics.median(base_rss), 'candidate_rss_median_bytes': statistics.median(cand_rss),
              'rss_delta_bytes': summarize(rss_delta, 6101201+fixtures.index(fixture)),
              'all_outputs_equal': all(r['baseline']['semantic_sha256']==r['candidate']['semantic_sha256'] for r in rows)}}
    (work / f'{name}-paired.json').write_text(json.dumps(report, indent=2)+'\n')
    all_reports[fixture] = report['summary']
    print(f'{fixture} SUMMARY '+json.dumps(report['summary'], sort_keys=True), flush=True)
(work / 'remaining-fixtures-paired-summary.json').write_text(json.dumps(all_reports, indent=2)+'\n')
