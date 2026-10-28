#!/usr/bin/env python3
"""11 alternating pinned production A/B pairs for Task 61 descriptor transitions."""
import csv
import hashlib
import re
import subprocess
import time
from pathlib import Path

ROOT = Path.cwd()
BASELINE = ROOT / 'target/iteration/task61-code-cursor-candidate-20261009/production/quench-node'
CANDIDATE = ROOT / 'target/iteration/task61-descriptor-transition-20261009/production/quench-node'
FIXTURE = ROOT / 'target/iteration/task61-argument-shapes-profile-20261009/materialized-earley-boyer.js'
CSV_PATH = ROOT / 'tasks/evidence/task61-earley-descriptor-transition-pairs-2026-10-09.csv'
EXPECTED = {
    BASELINE: '9175e3ac9ef03874f5a9d046963645ced1f427a0c8bd21de6b10632090e54783',
    CANDIDATE: 'eabe4cb354506920ef9ef217bcc935403ec49b2d61748b8ff476b0efde334d28',
    FIXTURE: 'aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b',
}
INSTRUCTIONS = re.compile(r'^\s*(\d+)\s+instructions retired\s*$', re.M)
RSS = re.compile(r'^\s*(\d+)\s+maximum resident set size\s*$', re.M)
ELAPSED = re.compile(r'^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$', re.M)
SCORE = re.compile(r'^Score:\s*(-?[\d.]+)\s*$', re.M)
MEASURE_FIELDS = ('score', 'instructions', 'max_rss_bytes', 'elapsed_seconds', 'user_seconds', 'system_seconds')


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalized_output(stdout: str) -> str:
    return '\n'.join(
        line for line in stdout.splitlines()
        if not line.startswith('__quenchBenchResult:')
        and line.strip() != '----'
        and not line.startswith('Score:')
    )


def run(binary: Path) -> dict[str, object]:
    result = subprocess.run(
        ['/usr/bin/time', '-l', str(binary), str(FIXTURE)],
        text=True,
        capture_output=True,
        check=False,
    )
    instruction_match = INSTRUCTIONS.search(result.stderr)
    rss_match = RSS.search(result.stderr)
    elapsed_match = ELAPSED.search(result.stderr)
    score_match = SCORE.search(result.stdout)
    if not all((instruction_match, rss_match, elapsed_match, score_match)):
        raise RuntimeError(
            f'missing parsed field for {binary}: exit={result.returncode}; '
            f'stdout={result.stdout!r}; stderr={result.stderr!r}'
        )
    elapsed, user, system = map(float, elapsed_match.groups())
    normalized = normalized_output(result.stdout)
    return {
        'exit': result.returncode,
        'score': float(score_match.group(1)),
        'instructions': int(instruction_match.group(1)),
        'max_rss_bytes': int(rss_match.group(1)),
        'elapsed_seconds': elapsed,
        'user_seconds': user,
        'system_seconds': system,
        'stdout': result.stdout,
        'stderr': result.stderr,
        'normalized_stdout': normalized,
        'stdout_sha256': sha256(result.stdout.encode()),
        'normalized_stdout_sha256': sha256(normalized.encode()),
    }


for path, expected_hash in EXPECTED.items():
    actual_hash = sha256(path.read_bytes())
    if actual_hash != expected_hash:
        raise SystemExit(f'pin mismatch for {path}: expected {expected_hash}, got {actual_hash}')

started = time.time()
rows: list[dict[str, object]] = []
for pair in range(1, 12):
    order = ('candidate', 'baseline') if pair % 2 else ('baseline', 'candidate')
    measured = {}
    for label in order:
        binary = CANDIDATE if label == 'candidate' else BASELINE
        measured[label] = run(binary)
    row: dict[str, object] = {'pair': pair, 'order': '>'.join(order)}
    for label in ('baseline', 'candidate'):
        row[f'{label}_binary_sha256'] = EXPECTED[BASELINE if label == 'baseline' else CANDIDATE]
        for field in MEASURE_FIELDS:
            row[f'{label}_{field}'] = measured[label][field]
        row[f'{label}_exit'] = measured[label]['exit']
        row[f'{label}_stdout_sha256'] = measured[label]['stdout_sha256']
        row[f'{label}_normalized_stdout_sha256'] = measured[label]['normalized_stdout_sha256']
        row[f'{label}_stdout'] = measured[label]['stdout']
        row[f'{label}_time_stderr'] = measured[label]['stderr']
    row['normalized_outputs_match'] = (
        measured['baseline']['normalized_stdout'] == measured['candidate']['normalized_stdout']
    )
    rows.append(row)
    print(f'pair {pair}/11 complete', flush=True)

CSV_PATH.parent.mkdir(parents=True, exist_ok=True)
fields = ['pair', 'order']
for label in ('baseline', 'candidate'):
    fields.extend([f'{label}_binary_sha256', *[f'{label}_{field}' for field in MEASURE_FIELDS],
                   f'{label}_exit', f'{label}_stdout_sha256', f'{label}_normalized_stdout_sha256',
                   f'{label}_stdout', f'{label}_time_stderr'])
fields.append('normalized_outputs_match')
with CSV_PATH.open('w', newline='') as stream:
    writer = csv.DictWriter(stream, fieldnames=fields)
    writer.writeheader()
    writer.writerows(rows)
print(f'csv={CSV_PATH}')
print(f'elapsed_wall_seconds={time.time() - started:.3f}')
