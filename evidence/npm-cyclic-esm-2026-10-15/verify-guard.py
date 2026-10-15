"""Check fixed-work completion independently of Callgrind's instruction totals."""
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import subprocess
import sys

baseline, candidate, directory = sys.argv[1:]
directory = Path(directory)
jobs = [(side, engine, script)
        for side, engine in [('base', baseline), ('cand', candidate)]
        for script in sorted(directory.glob('*.js'))]

def verify(job):
    side, engine, script = job
    fixture, runs = script.stem.rsplit('-', 1)
    row = dict(side=side, script=script.name)
    try:
        result = subprocess.run([engine, str(script)], capture_output=True,
                                text=True, timeout=60)
        row.update(exit_code=result.returncode, stdout=result.stdout,
                   stderr=result.stderr,
                   passed=result.returncode == 0 and not result.stderr
                   and result.stdout == f'done {fixture} {runs}\n')
    except subprocess.TimeoutExpired:
        row.update(passed=False, timed_out=True)
    return row

with ThreadPoolExecutor(2) as workers:
    rows = list(workers.map(verify, jobs))
(directory / 'completions.json').write_text(json.dumps(rows, indent=2) + '\n')
passed = sum(row['passed'] for row in rows)
print(json.dumps(dict(total=len(rows), passed=passed)))
sys.exit(0 if len(rows) == 32 and passed == 32 else 1)
