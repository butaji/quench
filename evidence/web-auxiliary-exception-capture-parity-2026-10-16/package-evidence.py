from pathlib import Path
import json,hashlib,subprocess,shutil
prefix='process-auxiliary-capture'
b=json.loads(Path(f'work/{prefix}-qualification-before.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','integrated-workspace','integrated-runtime','framework','web','final-process_capture','final-process_auxiliary_capture','final-process_uncaught']:assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0',name
assert '399 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text();assert '672 passed; 0 failed' in Path(f'work/{prefix}-integrated-runtime.log').read_text()
root=Path('evidence/web-auxiliary-exception-capture-parity-2026-10-16');root.mkdir(exist_ok=True)
for family in [prefix+'-*','process-capture-auxiliary-*','process-capture-auxiliary-coercion-*','process-capture-method-shape-probe*','process-auxiliary-upstream-*','uncaught-monitor-fatal-*']:
 for p in Path('work').glob(family):
  if p.is_file():shutil.copy2(p,root/p.name)
shutil.copytree(Path('work')/(prefix+'-web'),root/'web',dirs_exist_ok=True)
for name in ['process-capture-auxiliary','process-capture-auxiliary-coercion','process-capture-method-shape']:
 (root/('oracle-'+name+'.js')).write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/test-'+name+'.js']))
shutil.copy2(__file__,root/'package-evidence.py');(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':399,'api_seconds':223.87,'runtime_tests':672,'runtime_seconds':50.20,'framework_fixtures':43,'web_correctness_targets':9,'total_node_parity':False,'throughput_win':False,'performance_qualified':False},indent=2)+'\n')
(root/'README.md').write_text(f'''# Auxiliary exception capture and ordinary public method shapes

Source {source} passes 399 serial stable Node v26.12.0 API comparisons (223.87s), 672 integrated Runtime tests (50.20s), workspace/all-target, 43 framework fixtures and all nine no-JIT web correctness targets. Before/after API manifests are identical. Later source is excluded. Parent 985f604b3 separately passes 672 Core tests (81.37s, concurrent profiling/build); shared f036cd787 eval/promise/provenance integration is rebuilt and rerun independently. No wall-time comparison is claimed.

Rust tries auxiliary callbacks in reverse order, accepts only literal true, and reloads the callback collection for each property call. Shared VM subtraction owns the initial length conversion; fractional keys and replacement during conversion remain observable. The retained Runtime adapter validates all input roots before guest effects, preserves coercion/GC order and retains original thrown values. Public set/has methods use Node's names and ordinary constructor shapes, independently of private dispatch IDs. Eight focused exception comparisons pass; integrated full APIs include them.

The broader upstream errors and primary-capture fixtures pass. The upstream monitor fixture still fails: a monitor listener throwing exits 7 on Node and 1 on Quench. Both direct outputs/statuses are retained as an open finding. Diagnostic report/abort/async hooks, internal auxiliary registration, source fidelity, broader Node parity and a framework win remain open. Frozen b6dfc3996 instruction evidence is separate and does not qualify this newer source's performance.
''');print(root)
