import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path('/workspace/quench')
SCRATCH = Path('/workspace/scratch')
PREFIX = SCRATCH / 'npm-fs-sync'
NODE = '/workspace/shared/node-v26.12.0-linux-x64/bin/node'
CANDIDATE = str(SCRATCH / 'npm-fs-sync-candidate')
BASELINE = str(SCRATCH / 'npm-canonical-sync-candidate')
RESUME_LABELS = {
    'node-lib', 'decoder-mixed-node', 'decoder-mixed-quench',
    'decoder-single-byte-node', 'decoder-single-byte-quench',
    'decoder-two-byte-node', 'decoder-two-byte-quench',
} if '--resume-verified-corpora' in sys.argv else set()

def run(label, args, cwd=ROOT, env=None):
    if label in RESUME_LABELS:
        if Path(f'{PREFIX}-{label}.exit').read_text().strip() != '0':
            raise RuntimeError(f'{label}: cannot resume an unsuccessful command')
        return 0
    with Path(f'{PREFIX}-{label}.log').open('wb') as output:
        result = subprocess.run(args, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT)
    Path(f'{PREFIX}-{label}.exit').write_text(str(result.returncode) + '\n')
    return result.returncode

def complete_report(label):
    report = json.loads(Path(f'{PREFIX}-{label}.json').read_text())
    if len(report['records']) != 300:
        raise RuntimeError(f'{label}: incomplete report')

def main():
    for executable, expected in [(BASELINE, 'd6482c1dbf34b2d0045f033404c6e469c916e893b7ceb8c205a610a7ce0681ff'),
                                 (NODE, 'caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943')]:
        if hashlib.sha256(Path(executable).read_bytes()).hexdigest() != expected:
            raise RuntimeError(f'executable changed: {executable}')
    completion = Path(f'{PREFIX}-opt-core.exit')
    while not completion.exists():
        for name in ['build-checks', 'fast-core', 'cli', 'node-api', 'node-lib', 'workspace-check']:
            receipt=Path(f'{PREFIX}-{name}.exit')
            if receipt.exists() and receipt.read_text().strip() != '0':
                raise RuntimeError(f'{name}: prerequisite failed')
        time.sleep(5)
    if completion.read_text().strip() != '0':
        raise RuntimeError('optimized Core gate failed')
    provenance=json.loads(Path(f'{PREFIX}-engines.json').read_text())
    if provenance['candidate']['source'] != 'b8b02f99245ee4e3392c84b75480733d7f135802':
        raise RuntimeError('unexpected frozen source')
    if hashlib.sha256(Path(CANDIDATE).read_bytes()).hexdigest() != provenance['candidate']['sha256']:
        raise RuntimeError('candidate changed after freezing')
    if provenance['node']['sha256'] != 'caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943':
        raise RuntimeError('unexpected API oracle')
    if Path(f'{PREFIX}-node-lib.exit').read_text().strip() != '0':
        raise RuntimeError('Node host library gate failed')
    packet = Path('/workspace/scratch/quench-npm-file-url/evidence/npm-esm-codecs-2026-10-15')
    for name, args in [
        ('decoder-mixed', []), ('decoder-single-byte', []), ('decoder-two-byte', []),
        ('url-wpt', [str(packet / 'urltestdata.json'), '/workspace/quench/tests/npm-top300/node_modules/whatwg-url']),
    ]:
        for side, engine in [('node', NODE), ('quench', CANDIDATE)]:
            if run(f'{name}-{side}', [engine, str(packet / f'{name}.cjs'), *args]):
                raise RuntimeError(f'{name}: {side} failed')
        if Path(f'{PREFIX}-{name}-node.log').read_bytes() != Path(f'{PREFIX}-{name}-quench.log').read_bytes():
            raise RuntimeError(f'{name}: oracle mismatch')
        output = Path(f'{PREFIX}-{name}-node.log').read_text()
        if name == 'decoder-mixed' and len(json.loads(output)) != 896:
            raise RuntimeError('mixed corpus completion missing')
        if name == 'decoder-single-byte' and sum(len(row[3]) for row in json.loads(output)) != 19968:
            raise RuntimeError('single-byte corpus completion missing')
        if name == 'decoder-two-byte':
            # JSON strings can contain literal Unicode line separators.
            # Only the LF written by console.log delimits these records.
            lines = output.rstrip('\n').split('\n')
            if len(lines) != 5121 or json.loads(lines[-1]) != {'complete': True, 'cases': 1310720}:
                raise RuntimeError('two-byte corpus completion missing')
        if name == 'url-wpt' and json.loads(output)['completed'] != 896:
            raise RuntimeError('URL corpus completion missing')
    runner = str(ROOT / 'target/iteration/run-npm')
    for label, engine, extra in [('scenarios', CANDIDATE, []), ('surface', CANDIDATE, ['--surface'])]:
        result = run(label, [runner, NODE, engine, f'{PREFIX}-{label}.json', '30', *extra])
        if result not in (0, 1):
            raise RuntimeError(f'{label}: runner failed with {result}')
        complete_report(label)
    # Reuse an exact retained baseline observation, with explicit lineage. This
    # is not a fresh execution; the frozen executable and harness are unchanged.
    baseline_path=SCRATCH/'npm-canonical-sync-scenarios.json'
    baseline=json.loads(baseline_path.read_text())
    candidate=json.loads(Path(f'{PREFIX}-scenarios.json').read_text())
    if len(baseline['records']) != 300 or baseline['quench_sha256'] != provenance['baseline']['sha256']:
        raise RuntimeError('retained baseline is incomplete or uses another executable')
    for field in ['inventory_sha256','lock_sha256','node_sha256','scenario_sha256']:
        if baseline[field] != candidate[field]:
            raise RuntimeError(f'retained baseline differs in {field}')
    Path(f'{PREFIX}-baseline-scenarios.json').write_bytes(baseline_path.read_bytes())
    Path(f'{PREFIX}-baseline-scenarios-reuse.json').write_text(json.dumps({
        'reused': True, 'fresh_execution': False, 'original_report': str(baseline_path),
        'original_report_sha256': hashlib.sha256(baseline_path.read_bytes()).hexdigest(),
        'source': provenance['baseline']['source'], 'published_merge': provenance['baseline']['published_merge'],
        'matched_harness_fields': ['inventory_sha256','lock_sha256','node_sha256','scenario_sha256'],
    },indent=2)+'\n')
    run('coverage', [str(ROOT / 'target/iteration/npm-coverage'), f'{PREFIX}-surface.json', f'{PREFIX}-scenarios.json', f'{PREFIX}-coverage.json'])
    guard = ROOT / 'target/npm-fs-sync-instruction-guard'
    env = os.environ.copy()
    env['VALGRIND_LIB'] = '/workspace/.local/usr/libexec/valgrind'
    if run('instruction-guard', ['bash', str(SCRATCH / 'npm-esm-codecs-callgrind-guard.sh'), BASELINE, CANDIDATE, str(guard)], env=env):
        raise RuntimeError('instruction guard failed')
    rows = []
    for side, engine in [('base', BASELINE), ('cand', CANDIDATE)]:
        for script in sorted(guard.glob('*.js')):
            fixture, work = script.stem.rsplit('-', 1)
            result = subprocess.run([engine, str(script)], capture_output=True, text=True, timeout=120)
            rows.append({'side': side, 'script': script.name, 'exit_code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr,
                         'passed': result.returncode == 0 and result.stdout == f'done {fixture} {work}\n' and result.stderr == ''})
    Path(f'{PREFIX}-guard-completions.json').write_text(json.dumps(rows, indent=2) + '\n')
    if len(rows) != 32 or not all(row['passed'] for row in rows):
        raise RuntimeError('guard completion verification failed')

try:
    main()
except Exception as error:
    Path(f'{PREFIX}-suite-error.txt').write_text(repr(error) + '\n')
    Path(f'{PREFIX}-suite.exit').write_text('1\n')
    raise
else:
    Path(f'{PREFIX}-suite.exit').write_text('0\n')
