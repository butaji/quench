import hashlib
import json
import os
import pathlib
import random
import statistics
import subprocess
import time

root = pathlib.Path('/workspace/quench')
work = root / 'target/stageb-candidates/compact-cells'
fixture = root / 'work/stageb-candidates/compact-cells/with-env-allocation.js'
binaries = {
    'boxed_vec': work / 'candidate-boxed-vec-quench-node',
    'boxed_slice': work / 'candidate-boxed-slice-quench-node',
}
node = '/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
env = {key: value for key, value in os.environ.items() if key in {'PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'TZ'}}
source = fixture.read_text()
source_hash = hashlib.sha256(source.encode()).hexdigest()

def run(label, pair, index):
    out = work / f'with-env-{pair:02d}-{index}-{label}.stdout'
    err = work / f'with-env-{pair:02d}-{index}-{label}.stderr'
    started = time.perf_counter_ns()
    pid = os.fork()
    if pid == 0:
        with out.open('wb') as stdout, err.open('wb') as stderr:
            os.dup2(stdout.fileno(), 1)
            os.dup2(stderr.fileno(), 2)
            os.execve(str(binaries[label]), [str(binaries[label]), '-e', source], env)
    _, status, usage = os.wait4(pid, 0)
    return {
        'status': os.waitstatus_to_exitcode(status),
        'elapsed_ns': time.perf_counter_ns() - started,
        'user_cpu_ns': int(usage.ru_utime * 1e9),
        'max_rss_bytes': usage.ru_maxrss * 1024,
        'stdout': out.read_text(errors='replace'),
        'stderr': err.read_text(errors='replace'),
        'stdout_sha256': hashlib.sha256(out.read_bytes()).hexdigest(),
        'stderr_sha256': hashlib.sha256(err.read_bytes()).hexdigest(),
    }

node_run = subprocess.run([node, '-e', source], capture_output=True, text=True, check=True, env=env)
assert node_run.stdout == '4501500\n', node_run.stdout
assert node_run.stderr == ''
rows = []
for pair in range(11):
    order = ('boxed_vec', 'boxed_slice') if pair % 2 == 0 else ('boxed_slice', 'boxed_vec')
    results = {label: run(label, pair + 1, index) for index, label in enumerate(order)}
    assert all(r['status'] == 0 and r['stdout'] == node_run.stdout and r['stderr'] == '' for r in results.values()), (pair, results)
    rows.append({
        'pair': pair + 1,
        'order': order,
        'boxed_vec': {key: value for key, value in results['boxed_vec'].items() if key not in {'stdout', 'stderr'}},
        'boxed_slice': {key: value for key, value in results['boxed_slice'].items() if key not in {'stdout', 'stderr'}},
    })
    print(f"pair {pair + 1}/11 elapsed_delta_ns={results['boxed_slice']['elapsed_ns'] - results['boxed_vec']['elapsed_ns']} cpu_delta_ns={results['boxed_slice']['user_cpu_ns'] - results['boxed_vec']['user_cpu_ns']} rss_delta={results['boxed_slice']['max_rss_bytes'] - results['boxed_vec']['max_rss_bytes']}", flush=True)

def bootstrap_ci(values, seed):
    rng = random.Random(seed)
    samples = sorted(statistics.median(values[rng.randrange(len(values))] for _ in values) for _ in range(50_000))
    return [samples[1249], samples[48_749]]

report = {
    'schema': 1,
    'experiment': 'captured with-environment allocation microbenchmark',
    'source_sha256': source_hash,
    'node_version': subprocess.check_output([node, '--version'], text=True).strip(),
    'node_checksum': node_run.stdout.strip(),
    'binary_sha256': {label: hashlib.sha256(path.read_bytes()).hexdigest() for label, path in binaries.items()},
    'pairs': rows,
}
summary = {}
for metric in ['elapsed_ns', 'user_cpu_ns', 'max_rss_bytes']:
    values = [row['boxed_slice'][metric] - row['boxed_vec'][metric] for row in rows]
    summary[metric + '_delta_median'] = statistics.median(values)
    summary[metric + '_delta_95_ci'] = bootstrap_ci(values, 5_108_100 + len(metric))
report['summary'] = summary
(work / 'with-env-allocation-paired.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(summary, sort_keys=True))
