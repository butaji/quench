from pathlib import Path
import json,hashlib,subprocess,shutil
prefix='process-fatal-handler-exit'
b=json.loads(Path(f'work/{prefix}-qualification-before.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','final-workspace','framework','web','focused-api','paired-callgrind']:assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0',name
for name in ['test-process-exception-capture-errors.js','test-process-exception-capture.js','test-process-uncaught-exception-monitor.js']:assert Path('work/'+prefix+'-upstream-'+name+'.exit').read_text().strip()=='0'
assert '400 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text();assert not subprocess.check_output(['git','diff','2fdb67fcb',source,'--','crates/quench-runtime','crates/quench-regexp','Cargo.toml','Cargo.lock'])
r=json.loads(Path('work/'+prefix+'-paired-callgrind/report.json').read_text());assert r['all_valid'] and r['identities_unchanged'] and len(r['results'])==32
root=Path('evidence/web-fatal-handler-exit-parity-2026-10-16');root.mkdir(exist_ok=True)
for family in [prefix+'-*','uncaught-monitor-fatal-*','express-json-finite-batch*','cli-version-order-probe*']:
 for p in Path('work').glob(family):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind','load-diagnostic']:shutil.copytree(Path('work')/(prefix+'-'+name),root/name,dirs_exist_ok=True)
shutil.copytree('work/express-json-finite-batch',root/'finite-json-batch',dirs_exist_ok=True)
(root/'oracle-fatal-handler-exit.js').write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/test-process-fatal-handler-exit.js']))
shutil.copy2(__file__,root/'package-evidence.py')
(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':400,'api_seconds':223.86,'runtime_tests':672,'runtime_tree_verified_from':'2fdb67fcb','framework_fixtures':43,'web_correctness_targets':9,'fatal_child_cases':19,'upstream_cases_passed':3,'passed_instruction_guards':sum(c['within_limit']for c in r['comparisons']),'total_node_parity':False,'throughput_win':False,'source_only_build_matched_comparison':False},indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass'if c['within_limit']else'FAIL'} |"for c in r['comparisons'])
(root/'README.md').write_text(f'''# Fatal exception-handler exit state

Frozen source {source} passes 400 serial stable Node v26.12.0 API comparisons (223.86s), workspace/all-target, 43 framework fixtures and nine no-JIT web correctness targets. The 19-child oracle matrix and three upstream capture/monitor fixtures pass. API source/oracle/CLI/runner/test manifests are unchanged. Runtime/RegExp and Core Cargo inputs match the integrated 672-test passing 2fdb67fcb tree. Later source is excluded.

A handler failure is a distinct exit cause, with its status derived from the SDK's exit-code enum. It retains the thrown diagnostic, suppresses normal exit events and differs from explicit process.exit requests with codes 1, 7 and 23. The generated binding table uses those same typed enum variants instead of repeating numeric definitions. The matrix covers top-level, tick, promise and auxiliary errors, and explicit requests. Initial exit-1/exit-event failures are retained. The initial direct .js oracle invocation uses this repository's ESM package scope and fails require; the corrected .cjs invocation is separate.

All 32 fixed-work instruction captures are valid with unchanged identities; all eight guards pass against frozen 03ea582d2. Iteration opt1/256 CGUs/no LTO/incremental=false, shared-source and configuration differences prevent isolated source attribution or production throughput/RSS claims. The verifier's final working-tree revision may be newer; the frozen CLI hash and source manifest establish provenance.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}

The fresh one-second nine-target load diagnostic remains invalid for Quench Express JSON: 249 two-second timeouts. The other eight targets are valid in this short run. A separate finite 500-concurrent POST batch, with a 30-second deadline, completes every request with correct status/JSON and zero errors on Quench and Node. Quench takes 2571ms under concurrent profiling/build activity; this is a diagnostic observation, not a matched production timing comparison or proof of all traffic patterns.

This frozen CLI still prints the old hard-coded v22.0.0 although its bundled compatibility SDK is v26.12.0; the mismatch is retained in benchmark metadata. The later 6d20fba29 version/order correction is excluded. Report generation, abort/async-hook/source contracts, total Node parity and a framework benchmark win remain open. No authored performance tuning is claimed.
''');print(root)
