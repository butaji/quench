#!/usr/bin/env python3
import argparse
import hashlib
import gzip
import json
import math
import os
import platform
import random
import re
import signal
import statistics
import subprocess
import time
from pathlib import Path

ROOT = Path('/workspace/quench')
FIXTURES = ['earley-boyer.js']
PAIR_COUNT = 11
BOOTSTRAPS = 100_000
SEED = 20261009
TIMEOUT_SECONDS = 300


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_hash(path):
    return sha256(Path(path).read_bytes())


def percentile(values, probability):
    values = sorted(values)
    position = (len(values) - 1) * probability
    low = math.floor(position)
    high = math.ceil(position)
    if low == high:
        return values[low]
    return values[low] + (values[high] - values[low]) * (position - low)


def paired_bootstrap(baseline, candidate, rng):
    score_diffs = []
    for _ in range(BOOTSTRAPS):
        indices = [rng.randrange(PAIR_COUNT) for _ in range(PAIR_COUNT)]
        score_diffs.append(
            statistics.median(candidate[i] for i in indices)
            - statistics.median(baseline[i] for i in indices)
        )
    return {
        'lower_95': percentile(score_diffs, 0.025),
        'upper_95': percentile(score_diffs, 0.975),
        'resamples': BOOTSTRAPS,
        'method': 'paired percentile bootstrap of difference in medians',
        'seed': SEED,
    }


def materialize(fixture, output_dir):
    suite = ROOT / 'quench-bench/js-engine-benchmark/v8-v7'
    main_rs = (ROOT / 'quench-bench/src/main.rs').read_text()
    match = re.search(r'const RUNNER: &str = r#"(.*?)"#;', main_rs, re.S)
    if not match:
        raise RuntimeError('could not extract the pinned runner suffix')
    runner = match.group(1).encode()
    source = (suite / 'base.js').read_bytes() + b'\n' + (suite / fixture).read_bytes() + b'\n' + runner
    path = output_dir / fixture.replace('.js', '-materialized.js')
    path.write_bytes(source)
    return path, sha256(source)


def run_one(binary, input_path, environment, output_dir, sample_name):
    stdout_path = output_dir / f'{sample_name}.stdout'
    stderr_path = output_dir / f'{sample_name}.stderr'
    stdout_fd = os.open(stdout_path, os.O_CREAT | os.O_TRUNC | os.O_WRONLY, 0o600)
    stderr_fd = os.open(stderr_path, os.O_CREAT | os.O_TRUNC | os.O_WRONLY, 0o600)
    started = time.monotonic_ns()
    pid = os.fork()
    if pid == 0:
        try:
            os.setsid()
            os.dup2(stdout_fd, 1)
            os.dup2(stderr_fd, 2)
            os.close(stdout_fd)
            os.close(stderr_fd)
            os.execve(str(binary), [str(binary), str(input_path)], environment)
        except BaseException as error:
            os.write(2, f'exec failed: {error}\n'.encode())
            os._exit(127)
    os.close(stdout_fd)
    os.close(stderr_fd)
    deadline = time.monotonic() + TIMEOUT_SECONDS
    timed_out = False
    while True:
        waited, status, usage = os.wait4(pid, os.WNOHANG)
        if waited == pid:
            break
        if time.monotonic() >= deadline:
            timed_out = True
            try:
                os.killpg(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            grace = time.monotonic() + 1
            while time.monotonic() < grace:
                waited, status, usage = os.wait4(pid, os.WNOHANG)
                if waited == pid:
                    break
                time.sleep(0.02)
            else:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                _, status, usage = os.wait4(pid, 0)
            break
        time.sleep(0.01)
    stdout = stdout_path.read_text(errors='replace')
    stderr = stderr_path.read_text(errors='replace')
    score = None
    for line in stdout.splitlines():
        if line.startswith('Score: '):
            score = float(line[len('Score: '):])
            break
    result_lines = [line for line in stdout.splitlines() if line.startswith('__quenchBenchResult: ')]
    semantic_lines = [
        line for line in stdout.splitlines()
        if not line.startswith('Score: ')
        and line != '----'
        and not line.startswith('__quenchBenchResult: ')
    ]
    exit_code = os.waitstatus_to_exitcode(status)
    elapsed_ns = time.monotonic_ns() - started
    return {
        'command': [str(binary), str(input_path)],
        'exit_code': exit_code,
        'timed_out': timed_out,
        'score': score,
        'maximum_rss_bytes': int(usage.ru_maxrss) * 1024,
        'wall_ns': elapsed_ns,
        'result_lines': result_lines,
        'semantic_output': semantic_lines,
        'stdout': stdout,
        'stderr': stderr,
    }


def median(values):
    return statistics.median(values)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--candidate', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    materialized_dir = args.output.parent / 'materialized'
    materialized_dir.mkdir(parents=True, exist_ok=True)
    raw_dir = args.output.parent / 'samples'
    raw_dir.mkdir(parents=True, exist_ok=True)
    pair_log = args.output.with_name(args.output.name + '.pairs.jsonl')
    existing_records = {}
    if pair_log.exists():
        for line in pair_log.read_text().splitlines():
            item = json.loads(line)
            existing_records.setdefault(item['fixture'], []).append(item['record'])
    raw_report = json.loads((ROOT / 'tasks/evidence/task61-lazy-host-strings-linux-all8-one-round-2026-10-09.json').read_text())
    environment = next(engine['environment'] for engine in raw_report['engines'] if engine['name'] == 'quench')
    rng = random.Random(SEED)
    fixture_records = {}
    overall_valid = True
    for fixture in FIXTURES:
        input_path, materialized_hash = materialize(fixture, materialized_dir)
        records = existing_records.get(fixture, [])
        for pair in range(len(records), PAIR_COUNT):
            order = ['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline']
            results = {}
            for variant in order:
                binary = args.baseline if variant == 'baseline' else args.candidate
                name = f'{fixture.removesuffix(".js")}-pair-{pair + 1}-{variant}'
                results[variant] = run_one(binary, input_path, environment, raw_dir, name)
            baseline = results['baseline']
            candidate = results['candidate']
            pair_valid = all(
                result['exit_code'] == 0 and not result['timed_out']
                and result['score'] is not None and math.isfinite(result['score'])
                and result['maximum_rss_bytes'] > 0
                for result in results.values()
            )
            outputs_equal = baseline['semantic_output'] == candidate['semantic_output']
            pair_valid = pair_valid and outputs_equal
            overall_valid = overall_valid and pair_valid
            records.append({
                'pair': pair + 1,
                'execution_order': order,
                'valid': pair_valid,
                'output_equal': outputs_equal,
                'results': results,
            })
            with pair_log.open('a') as log:
                log.write(json.dumps({'fixture': fixture, 'record': records[-1]}, separators=(',', ':')) + '\n')
                log.flush()
                os.fsync(log.fileno())
            print(f'{fixture}: pair {pair + 1}/{PAIR_COUNT} order={order} valid={pair_valid}', flush=True)
        baseline_scores = [row['results']['baseline']['score'] for row in records]
        candidate_scores = [row['results']['candidate']['score'] for row in records]
        baseline_rss = [row['results']['baseline']['maximum_rss_bytes'] for row in records]
        candidate_rss = [row['results']['candidate']['maximum_rss_bytes'] for row in records]
        fixture_records[fixture] = {
            'source_sha256': file_hash(ROOT / 'quench-bench/js-engine-benchmark/v8-v7' / fixture),
            'materialized_sha256': materialized_hash,
            'materialized_path': str(input_path.relative_to(ROOT)),
            'valid': all(row['valid'] for row in records),
            'output_equal': all(row['output_equal'] for row in records),
            'rounds': records,
            'summary': {
                'baseline_median_score': median(baseline_scores),
                'candidate_median_score': median(candidate_scores),
                'score_delta': median(candidate_scores) - median(baseline_scores),
                'score_delta_percent': (median(candidate_scores) / median(baseline_scores) - 1) * 100,
                'score_delta_95_percentile_interval': paired_bootstrap(baseline_scores, candidate_scores, rng),
                'baseline_median_maximum_rss_bytes': median(baseline_rss),
                'candidate_median_maximum_rss_bytes': median(candidate_rss),
                'maximum_rss_delta_bytes': median(candidate_rss) - median(baseline_rss),
                'maximum_rss_delta_95_percentile_interval': paired_bootstrap(baseline_rss, candidate_rss, rng),
                'candidate_lower_score_pairs': sum(c < b for b, c in zip(baseline_scores, candidate_scores)),
                'candidate_lower_rss_pairs': sum(c < b for b, c in zip(baseline_rss, candidate_rss)),
            },
        }
    cpu_max = Path('/sys/fs/cgroup/cpu.max').read_text().strip()
    memory_max = Path('/sys/fs/cgroup/memory.max').read_text().strip()
    report = {
        'schema': 1,
        'task': '61',
        'experiment': 'V2 bounded dense array holes, isolated Linux EarleyBoyer production pairs',
        'source': {
            'baseline_revision': subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
            'candidate_base_revision': subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
            'candidate_dirty': bool(subprocess.check_output(['git', '-C', str(ROOT), 'status', '--porcelain'], text=True).strip()),
            'candidate_patch_sha256': sha256(gzip.decompress((Path(__file__).with_name('task61-dense-hole-array-candidate-linux-2026-10-09.patch.gz')).read_bytes())),
        },
        'host': {
            'uname': platform.uname()._asdict(),
            'rustc': raw_report['host']['rustc'],
            'cpu_quota': cpu_max,
            'memory_limit_bytes': int(memory_max),
            'rss_backend': 'Linux wait4 ru_maxrss, KiB normalized to bytes',
        },
        'corpus_revision': subprocess.check_output(['git', '-C', str(ROOT / 'quench-bench/js-engine-benchmark'), 'rev-parse', 'HEAD'], text=True).strip(),
        'runner_sha256': file_hash(Path(__file__)),
        'rounds_per_fixture': PAIR_COUNT,
        'bootstrap_resamples': BOOTSTRAPS,
        'binary_sha256': {'baseline': file_hash(args.baseline), 'candidate': file_hash(args.candidate)},
        'commands': {'baseline': [str(args.baseline), '<materialized-fixture>'], 'candidate': [str(args.candidate), '<materialized-fixture>']},
        'measurement_environment': environment,
        'fixtures': fixture_records,
        'valid': overall_valid,
        'qualification_ready': False,
        'decision_rule': 'Report paired Score and maximum-RSS intervals per affected fixture; output must match. This Quench-only screen does not establish Task 61 engine leadership.',
    }
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(f'WROTE {args.output} valid={overall_valid}', flush=True)
    if not overall_valid:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
