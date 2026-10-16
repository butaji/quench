from pathlib import Path
import json,hashlib,shutil,subprocess
prefix='process-capture-canonical-state'
b=json.loads(Path(f'work/{prefix}-qualification-before.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','integrated-workspace','framework','web','focused-api','root-lifecycle']:assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0',name
assert '393 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text()
assert not subprocess.check_output(['git','diff','f4e6ed305',source,'--','crates/quench-runtime','crates/quench-regexp','Cargo.toml','Cargo.lock'])
root=Path('evidence/web-canonical-exception-capture-parity-2026-10-16');root.mkdir(exist_ok=True)
for p in Path('work').glob(prefix+'-*'):
 if p.is_file():shutil.copy2(p,root/p.name)
for prefix2 in ['process-capture-unbound-probe','process-capture-state-probe','process-capture-this','process-capture-canonical-state-fixture']:
 for p in Path('work').glob(prefix2+'*'):
  if p.is_file():shutil.copy2(p,root/p.name)
shutil.copytree(Path('work')/(prefix+'-web'),root/'web',dirs_exist_ok=True)
shutil.copy2('crates/quench-node-test/node-tests/test-process-capture-canonical-state.js',root/'oracle-canonical-capture.js')
shutil.copy2(__file__,root/'package-evidence.py')
(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':393,'runtime_tests':671,'runtime_tree_verified_from':'f4e6ed305','root_lifecycle_test_passed':True,'root_lifecycle_source':'bae751c98','framework_fixtures':43,'web_correctness_targets':9,'total_node_parity':False,'throughput_win':False,'performance_qualified':False},indent=2)+'\n')
(root/'README.md').write_text(f'''# Canonical exception capture ownership

Source {source} passes 393 serial Node v26.12.0 API comparisons, workspace/all-target, 43 framework fixtures and nine no-JIT web correctness targets. API source/oracle/CLI/runner/test-binary manifests are unchanged. Runtime and RegExp source plus Core Cargo inputs match the f4e6ed305 tree whose 671 Runtime tests pass. The separate root lifecycle regression passes at bae751c98: the captured state survives GC and expires on a fresh VM execution. Subsequent integration changes only Node regex features/link order and evidence; full integrated APIs were rebuilt and rerun.

Rust owns a retained state object instead of a forgeable process string property. Set/has ignore their this receiver; null reset and argument/error priority are preserved. Delivery uses the same state receiver, whose captureFn mutation affects has/set immediately. Permanent oracle coverage checks undefined/null/object/process receivers, GC, marker forgery, receiver shape and mutation. Existing GC/reset host test is expanded rather than duplicating its setup.

The initial strict-mode callback inspection incorrectly used arguments.callee and throws on the oracle; it is retained separately from the corrected named callback inspection. The original report probe exposes missing process.report, not a successful report flag check. Report lifecycle, auxiliary registration, monitor ordering, capture getter re-read ordering, host stacks, total parity and a benchmark win remain open. No production performance or optimization claim accompanies this bounded correctness change.
''')
print(root)
