#!/usr/bin/env python3
import hashlib, json, os, re, signal, statistics, random, subprocess, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent
BASELINE = ROOT / 'work/task61-earley-construct-inline-args-linux-2026-10-10/baseline'
CANDIDATE = ROOT / 'target/production/quench-node'
FIXTURE = 'earley-boyer'
ROUNDS = 11
BOOTSTRAP = 100_000

def sha(data):
    return hashlib.sha256(data).hexdigest()

def materialize():
    pinned = ROOT / 'tasks/evidence/task61-earley-gc-headroom-3-4-recheck-linux-2026-10-10/materialized-earley-boyer.js'
    output = EVIDENCE / 'materialized-earley-boyer.js'
    output.write_bytes(pinned.read_bytes())
    return output

def run(binary, input_path, env, label):
    stdout_path = EVIDENCE / f'{label}.stdout'
    stderr_path = EVIDENCE / f'{label}.stderr'
    out_fd = os.open(stdout_path, os.O_CREAT|os.O_TRUNC|os.O_WRONLY, 0o600)
    err_fd = os.open(stderr_path, os.O_CREAT|os.O_TRUNC|os.O_WRONLY, 0o600)
    started = time.monotonic_ns()
    pid = os.fork()
    if pid == 0:
        try:
            os.setsid(); os.dup2(out_fd, 1); os.dup2(err_fd, 2)
            os.close(out_fd); os.close(err_fd)
            os.execve(str(binary), [str(binary), str(input_path)], env)
        except BaseException as error:
            os.write(2, f'exec failed: {error}\n'.encode()); os._exit(127)
    os.close(out_fd); os.close(err_fd)
    deadline = time.monotonic() + 300
    while True:
        waited, status, usage = os.wait4(pid, os.WNOHANG)
        if waited == pid:
            break
        if time.monotonic() >= deadline:
            try: os.killpg(pid, signal.SIGTERM)
            except ProcessLookupError: pass
            _, status, usage = os.wait4(pid, 0); break
        time.sleep(.01)
    stdout = stdout_path.read_text(errors='replace')
    stderr = stderr_path.read_text(errors='replace')
    scores = re.findall(r'^Score: ([+-]?[0-9]+(?:\.[0-9]+)?)$', stdout, re.M)
    semantic = [line for line in stdout.splitlines()
                if not line.startswith('Score: ') and line != '----'
                and not line.startswith('__quenchBenchResult: ')]
    return {'exit_code': os.waitstatus_to_exitcode(status),
            'score': float(scores[0]) if len(scores) == 1 else None,
            'maximum_rss_bytes': int(usage.ru_maxrss)*1024,
            'wall_ns': time.monotonic_ns()-started,
            'semantic_output': semantic, 'stdout': stdout, 'stderr': stderr}

def bootstrap_median_interval(values, seed):
    rng = random.Random(seed)
    draws = sorted(statistics.median(rng.choices(values, k=len(values)))
                   for _ in range(BOOTSTRAP))
    return [draws[int(.025*BOOTSTRAP)], draws[int(.975*BOOTSTRAP)]]

def main():
    report_path = ROOT / 'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json'
    measured = json.loads(report_path.read_text())
    env = next(engine['environment'] for engine in measured['engines']
               if engine['name'] == 'quench')
    fixture = materialize()
    rows = []
    raw_path = EVIDENCE / 'pairs.jsonl'
    with raw_path.open('w') as raw:
        for pair in range(1, ROUNDS+1):
            order = ['candidate', 'baseline'] if pair % 2 else ['baseline', 'candidate']
            results = {}
            for variant in order:
                binary = CANDIDATE if variant == 'candidate' else BASELINE
                results[variant] = run(binary, fixture, env,
                                       f'pair{pair:02d}-{variant}')
            baseline, candidate = results['baseline'], results['candidate']
            equal = baseline['semantic_output'] == candidate['semantic_output']
            row = {'pair': pair, 'order': order, 'valid': equal and
                   all(result['exit_code'] == 0 and result['score'] is not None
                       for result in results.values()),
                   'output_equal': equal,
                   'baseline_score': baseline['score'],
                   'candidate_score': candidate['score'],
                   'score_delta': candidate['score']-baseline['score'],
                   'baseline_rss_bytes': baseline['maximum_rss_bytes'],
                   'candidate_rss_bytes': candidate['maximum_rss_bytes'],
                   'rss_delta_bytes': candidate['maximum_rss_bytes']-baseline['maximum_rss_bytes'],
                   'results': results}
            rows.append(row)
            raw.write(json.dumps(row, separators=(',', ':'))+'\n')
            raw.flush(); os.fsync(raw.fileno())
            print(f"pair {pair}/{ROUNDS}: score {baseline['score']}->{candidate['score']}; "
                  f"RSS {baseline['maximum_rss_bytes']}->{candidate['maximum_rss_bytes']}; "
                  f"output_equal={equal}", flush=True)
    if not all(row['valid'] for row in rows):
        raise SystemExit('invalid or output-mismatched pair; refusing summary')
    print('baseline_sha256', sha(BASELINE.read_bytes()))
    print('candidate_sha256', sha(CANDIDATE.read_bytes()))
    print('fixture_sha256', sha(fixture.read_bytes()))
    print('rounds', ROUNDS, 'bootstrap_replicates', BOOTSTRAP)

if __name__ == '__main__':
    main()
