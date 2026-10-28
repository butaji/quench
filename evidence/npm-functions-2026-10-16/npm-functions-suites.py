import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path('/workspace/quench')
SCRATCH = Path('/workspace/scratch')
PREFIX = SCRATCH / 'npm-functions'
NODE = '/workspace/shared/node-v26.12.0-linux-x64/bin/node'
CANDIDATE = str(SCRATCH / 'npm-functions-candidate')
BASELINE = str(SCRATCH / 'npm-fs-sync-candidate')
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
    for executable, expected in [(BASELINE, '8bf0b426e7f8cf23df979d055b84d91c21e6ca5878f88590618aa1773a07c7fe'),
                                 (NODE, 'caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943')]:
        if hashlib.sha256(Path(executable).read_bytes()).hexdigest() != expected:
            raise RuntimeError(f'executable changed: {executable}')
    completion = Path(f'{PREFIX}-opt-core.exit')
    while not completion.exists():
        for name in ['build-checks', 'fast-core', 'cli', 'runners', 'coverage-unit', 'node-api', 'node-lib', 'workspace-check']:
            receipt=Path(f'{PREFIX}-{name}.exit')
            if receipt.exists() and receipt.read_text().strip() != '0':
                raise RuntimeError(f'{name}: prerequisite failed')
        time.sleep(5)
    if completion.read_text().strip() != '0':
        raise RuntimeError('optimized Core gate failed')
    provenance=json.loads(Path(f'{PREFIX}-engines.json').read_text())
    if provenance['candidate']['source'] != 'cac5edc64d41ce6e76b0bd800e193fbb14493f4b':
        raise RuntimeError('unexpected frozen source')
    if hashlib.sha256(Path(CANDIDATE).read_bytes()).hexdigest() != provenance['candidate']['sha256']:
        raise RuntimeError('candidate changed after freezing')
    if provenance['node']['sha256'] != 'caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943':
        raise RuntimeError('unexpected API oracle')
    if Path(f'{PREFIX}-node-lib.exit').read_text().strip() != '0':
        raise RuntimeError('Node host library gate failed')
    for label, script, complete_cases in [
        ('math-bit-dense', SCRATCH/'npm-node-math-bit-dense.cjs', 36065),
        ('class-length', SCRATCH/'npm-class-function-length.cjs', None),
    ]:
        for side, engine in [('node',NODE),('quench',CANDIDATE)]:
            if run(f'{label}-{side}',[engine,str(script)]):
                raise RuntimeError(f'{label}: {side} failed')
        left=Path(f'{PREFIX}-{label}-node.log').read_bytes()
        right=Path(f'{PREFIX}-{label}-quench.log').read_bytes()
        result=json.loads(left)
        if left != right or result['complete'] is not True:
            raise RuntimeError(f'{label}: exact oracle mismatch or incomplete run')
        if complete_cases is not None and result['cases'] != complete_cases:
            raise RuntimeError(f'{label}: incomplete case count')
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
    runner = '/workspace/quench/target/npm-file-url/iteration/run-npm'
    for label, engine, extra in [('scenarios', CANDIDATE, []), ('baseline-scenarios', BASELINE, []), ('surface', CANDIDATE, ['--surface'])]:
        result = run(label, [runner, NODE, engine, f'{PREFIX}-{label}.json', '30', *extra])
        if result not in (0, 1):
            raise RuntimeError(f'{label}: runner failed with {result}')
        complete_report(label)
    run('coverage', ['/workspace/quench/target/npm-file-url/iteration/npm-coverage', f'{PREFIX}-surface.json', f'{PREFIX}-scenarios.json', f'{PREFIX}-coverage.json'])
    guard = ROOT / 'target/npm-functions-instruction-guard'
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
