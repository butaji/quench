"""Run the pinned upstream cases with an explicit completion-count adapter.

Usage: python3 verify.py NODE QUENCH STANDALONE_NODE_ROOT OUTPUT_DIR
The standalone Node checkout prevents this repository's package scope from
changing the unchanged upstream CommonJS child fixture into an ES module.
"""
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile

node, quench, root, output = map(pathlib.Path, sys.argv[1:])
out = output.resolve()
out.mkdir(parents=True, exist_ok=True)
source = root.resolve() / 'test/parallel/test-parse-args.mjs'
original = source.read_text()
expected = '8711f0348ab12f36d86a79c46084508b72a38553a2c35ae6c1e1bbb26a86bdd0'
assert hashlib.sha256(source.read_bytes()).hexdigest() == expected
harness = '''const cases = [];
function suite(name, fn) { fn(); }
function test(name, fn) { cases.push([name, fn]); }
'''
adapted = original.replace("import { suite, test } from 'node:test';", harness)
for name in ['index', 'fixtures']:
    adapted = adapted.replace(f"'../common/{name}.mjs'", repr(str(root.resolve() / f'test/common/{name}.mjs')))
adapted += '''\nlet passed = 0;
for (const [name, fn] of cases) {
  try { await fn(); passed++; }
  catch (error) { console.error('FAIL', name, error.stack); }
}
console.log(JSON.stringify({registered: cases.length, passed}));
if (passed !== cases.length || passed !== 117) process.exitCode = 1;
'''
(out / 'upstream-completion.mjs').write_text(adapted)
repo = pathlib.Path(__file__).resolve().parents[2]
fixtures = repo / 'crates/quench-node-test/node-tests'
summary = {'upstream_sha256': expected, 'node_version': subprocess.check_output([node, '--version'], text=True).strip(), 'engines': {}, 'checks': {}}
for label, engine in [('node', node), ('quench', quench)]:
    summary['engines'][label] = {'path': str(engine), 'sha256': hashlib.sha256(engine.read_bytes()).hexdigest()}
    for kind, file in [('upstream', out / 'upstream-completion.mjs'), ('contract', fixtures / 'test-util-parse-args-contract.js'), ('generated', fixtures / 'test-util-parse-args-generated.js')]:
        # --input-type=commonjs keeps reusable JS fixtures independent of the
        # repository's type:module package scope; upstream remains a module.
        args = [engine, file] if kind == 'upstream' else [engine, '--input-type=commonjs', '-e', file.read_text()]
        result = subprocess.run(args, capture_output=True, timeout=180)
        (out / f'{kind}-{label}.stdout').write_bytes(result.stdout)
        (out / f'{kind}-{label}.stderr').write_bytes(result.stderr)
        summary['checks'][f'{kind}-{label}'] = {'exit': result.returncode, 'source_sha256': hashlib.sha256(file.read_bytes()).hexdigest()}
        assert result.returncode == 0, (kind, label, result.stderr)
for kind in ['upstream', 'contract', 'generated']:
    assert (out / f'{kind}-node.stdout').read_bytes() == (out / f'{kind}-quench.stdout').read_bytes(), kind
    assert (out / f'{kind}-node.stderr').read_bytes() == (out / f'{kind}-quench.stderr').read_bytes(), kind
assert json.loads((out / 'upstream-quench.stdout').read_text().splitlines()[-1]) == {'registered': 117, 'passed': 117}
assert len(json.loads((out / 'generated-quench.stdout').read_text())) == 1200
summary['qualified_scope'] = '117 upstream cases, 1200 generated cases, focused contracts; not universal util or package parity'
(out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(summary['qualified_scope'])
