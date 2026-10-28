import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path('/workspace/quench')
SCRATCH = Path('/workspace/scratch')
PREFIX = SCRATCH / 'npm-canonical-sync'
NODE = '/workspace/shared/node-v26.12.0-linux-x64/bin/node'
CANDIDATE = str(SCRATCH / 'npm-canonical-sync-candidate')
BASELINE = str(SCRATCH / 'npm-canonical-sync-baseline')
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
    for executable, expected in [(CANDIDATE, 'd6482c1dbf34b2d0045f033404c6e469c916e893b7ceb8c205a610a7ce0681ff'),
                                 (NODE, 'caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943')]:
        if hashlib.sha256(Path(executable).read_bytes()).hexdigest() != expected:
            raise RuntimeError(f'executable changed: {executable}')
    completion = Path(f'{PREFIX}-opt-core.exit')
    while not completion.exists():
        for name in ['fast-core', 'node-api', 'workspace-check']:
            receipt=Path(f'{PREFIX}-{name}.exit')
            if receipt.exists() and receipt.read_text().strip() != '0':
                raise RuntimeError(f'{name}: prerequisite failed')
        time.sleep(5)
    if completion.read_text().strip() != '0':
        raise RuntimeError('optimized Core gate failed')
    candidate_env = os.environ.copy()
    candidate_env.update(CARGO_TARGET_DIR='/workspace/quench/target/npm-file-url', CARGO_INCREMENTAL='0', PKG_CONFIG_PATH='/workspace/shared/icu-dev/root/usr/lib/x86_64-linux-gnu/pkgconfig')
    if run('node-lib', ['cargo', 'test', '-p', 'quench-node', '--lib'], cwd=Path('/workspace/scratch/quench-npm-file-url'), env=candidate_env):
        raise RuntimeError('Node host library tests failed')
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
    receipt=Path(f'{PREFIX}-baseline-cli.exit')
    while not receipt.exists():
        time.sleep(5)
    if receipt.read_text().strip() != '0' or not Path(BASELINE).is_file():
        raise RuntimeError('baseline build failed')
    source='fcc81c6a09b8ff4fd68e923885c62cdc9542b6bc'
    Path(f'{PREFIX}-engines.json').write_text(json.dumps({
        'candidate': {'source': 'faa120035a332d3b5eb232eaadcad1399f306d7d', 'sha256': hashlib.sha256(Path(CANDIDATE).read_bytes()).hexdigest()},
        'baseline': {'source': source, 'sha256': hashlib.sha256(Path(BASELINE).read_bytes()).hexdigest()},
        'node': {'version': '26.12.0', 'sha256': hashlib.sha256(Path(NODE).read_bytes()).hexdigest()},
    }, indent=2) + '\n')
    runner = str(ROOT / 'target/iteration/run-npm')
    for label, engine, extra in [('scenarios', CANDIDATE, []), ('baseline-scenarios', BASELINE, []), ('surface', CANDIDATE, ['--surface'])]:
        result = run(label, [runner, NODE, engine, f'{PREFIX}-{label}.json', '30', *extra])
        if result not in (0, 1):
            raise RuntimeError(f'{label}: runner failed with {result}')
        complete_report(label)
    run('coverage', [str(ROOT / 'target/iteration/npm-coverage'), f'{PREFIX}-surface.json', f'{PREFIX}-scenarios.json', f'{PREFIX}-coverage.json'])
    guard = ROOT / 'target/npm-canonical-sync-instruction-guard'
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
