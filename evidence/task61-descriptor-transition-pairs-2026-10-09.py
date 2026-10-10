"""Same alternating /usr/bin/time runner used for Task 61 closure pairs, pinned for descriptor candidate."""
import csv
import hashlib
import re
import subprocess
from pathlib import Path

ROOT = Path.cwd()
BASELINE = ROOT / 'target/iteration/task61-code-cursor-candidate-20261009/production/quench-node'
CANDIDATE = ROOT / 'target/iteration/task61-descriptor-transition-20261009/production/quench-node'
FIXTURE = ROOT / 'tasks/evidence/task61-normal-closure-10m-2026-10-09.cjs'
OUTPUT = ROOT / 'tasks/evidence/task61-descriptor-transition-pairs-2026-10-09.csv'
INSTRUCTIONS = re.compile(r'^\s*(\d+)\s+instructions retired\s*$', re.M)
RSS = re.compile(r'^\s*(\d+)\s+maximum resident set size\s*$', re.M)
ELAPSED = re.compile(r'^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$', re.M)
sha = lambda data: hashlib.sha256(data).hexdigest()

rows = []
for pair in range(1, 12):
    order = ('candidate', 'baseline') if pair % 2 else ('baseline', 'candidate')
    measurements = {}
    for label in order:
        binary = CANDIDATE if label == 'candidate' else BASELINE
        result = subprocess.run(['/usr/bin/time', '-l', str(binary), str(FIXTURE)], text=True, capture_output=True)
        if result.returncode != 0:
            raise SystemExit(f'{label} failed in pair {pair}: exit={result.returncode}\n{result.stderr}')
        instructions = int(INSTRUCTIONS.search(result.stderr).group(1))
        rss = int(RSS.search(result.stderr).group(1))
        elapsed, user, system = map(float, ELAPSED.search(result.stderr).groups())
        measurements[label] = {
            'instructions': instructions,
            'max_rss_bytes': rss,
            'elapsed_seconds': elapsed,
            'user_seconds': user,
            'system_seconds': system,
            'exit': result.returncode,
            'stdout_bytes': len(result.stdout.encode()),
            'stdout_sha256': sha(result.stdout.encode()),
        }
    rows.append({
        'pair': pair,
        'order': '>'.join(order),
        **{f'{label}_{field}': value for label, data in measurements.items() for field, value in data.items()},
        'outputs_match': measurements['baseline']['stdout_sha256'] == measurements['candidate']['stdout_sha256'],
    })
    print(f"pair {pair}/11 complete", flush=True)

OUTPUT.parent.mkdir(parents=True, exist_ok=True)
fields = ['pair', 'order'] + [f'{label}_{field}' for label in ('baseline', 'candidate') for field in ('instructions', 'max_rss_bytes', 'elapsed_seconds', 'user_seconds', 'system_seconds', 'exit', 'stdout_bytes', 'stdout_sha256')] + ['outputs_match']
with OUTPUT.open('w', newline='') as stream:
    writer = csv.DictWriter(stream, fieldnames=fields)
    writer.writeheader()
    writer.writerows(rows)
print(OUTPUT)
