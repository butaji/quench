from pathlib import Path
import json,hashlib,shutil,subprocess
prefix='net-lifecycle-integration'
b=json.loads(Path(f'work/{prefix}-qualification-before.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','workspace','runtime','framework','web']:assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0'
assert '392 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text()
assert '671 passed; 0 failed' in Path(f'work/{prefix}-runtime.log').read_text()
root=Path('evidence/web-native-lifecycle-shared-integration-2026-10-16');root.mkdir(exist_ok=True)
for p in Path('work').glob(prefix+'-*'):
 if p.is_file():shutil.copy2(p,root/p.name)
shutil.copytree(Path('work')/(prefix+'-web'),root/'web',dirs_exist_ok=True)
(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':392,'api_seconds':224.09,'runtime_tests':671,'runtime_seconds':75.75,'framework_fixtures':43,'web_correctness_targets':9,'total_node_parity':False,'throughput_win':False,'performance_qualified':False},indent=2)+'\n')
shutil.copy2(__file__,root/'package-evidence.py')
(root/'README.md').write_text(f'''# Native lifecycle shared integration

Published source {source} passes 392 serial Node v26.12.0 API comparisons (224.09 seconds), 671 Runtime tests (75.75 seconds), workspace/all-target checks, 43 framework fixtures and all nine no-JIT web correctness targets. Before/after manifests are identical: tracked source files, Node oracle, frozen CLI, mutable host CLI, runner and API-test binary. Git blobs verify the recorded source hashes. Later source is excluded.

The shared integration preserves both owners' appended tests and source policies, including the directory SDK and caller/protocol work. Its proof is independent of the earlier 387-case lifecycle snapshot. Runtime tests ran alongside compilation and a separate frozen executable's instruction profiler; no wall-time comparison is claimed. Full APIs ran serially after the profiler and heavy builds finished.

This is bounded compatibility evidence. Total stable Node parity remains open. The Express JSON load diagnostic still has request timeouts; no production throughput, RSS qualification or benchmark win is established. Earlier failed HTTP integration and instruction regressions remain separately retained.
''')
print(root)
