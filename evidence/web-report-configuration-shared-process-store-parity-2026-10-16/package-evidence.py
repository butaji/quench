from pathlib import Path
import json,hashlib,subprocess,shutil
prefix='process-report-shared-store'
b=json.loads(Path(f'work/{prefix}-qualification-before.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','workspace','framework','web']:assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0',name
assert '405 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text();assert not subprocess.check_output(['git','diff','2fdb67fcb',source,'--','crates/quench-runtime','crates/quench-regexp','Cargo.toml','Cargo.lock'])
root=Path('evidence/web-report-configuration-shared-process-store-parity-2026-10-16');root.mkdir(exist_ok=True)
for family in [prefix+'-*','report-configuration-*','process-shared-emitter-*','process-shared-event-store-*','process-report-configuration-*','process-report-startup-*','process-capture-report-flags-*','process-report-option-order-probe*','cli-version-order-*','process-warning-listener-policy-probe*']:
 for p in Path('work').glob(family):
  if p.is_file():shutil.copy2(p,root/p.name)
shutil.copytree(Path('work')/(prefix+'-web'),root/'web',dirs_exist_ok=True)
for name in ['process-report-configuration','process-capture-report-flags','process-report-startup','process-shared-event-store','cli-version-order']:
 (root/('oracle-'+name+'.js')).write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/test-'+name+'.js']))
shutil.copy2(__file__,root/'package-evidence.py');(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':405,'api_seconds':233.89,'runtime_tests':672,'runtime_tree_verified_from':'2fdb67fcb','framework_fixtures':43,'web_correctness_targets':9,'total_node_parity':False,'throughput_win':False,'performance_qualified':False},indent=2)+'\n')
(root/'README.md').write_text(f'''# SDK diagnostic-report configuration and a single process listener store

Source {source} passes 405 serial stable Node v26.12.0 API comparisons (233.89s), workspace/all-target, 43 framework fixtures and nine no-JIT web correctness targets. API source/oracle/CLI/runner/test manifests remain identical. Runtime/RegExp source and Core Cargo inputs match the integrated 672-test passing 2fdb67fcb tree. Later source is excluded.

Rust stores actual mutable diagnostic configuration, initialized through the shared CLI option owner. The pinned Node report module owns public descriptors, validation, source and signal-setter policy. Directory aliases, environment precedence and ordered report-signal implications match the oracle. Capture snapshots strict true, disables reporting, restores only for strict numeric-zero auxiliary length, evaluates restoration references in order and preserves slot changes before abrupt getters. Shared library identity is verified.

The new startup test exposed process's duplicate native listener map: registration worked but inherited listenerCount saw zero. Process now uses the common EventEmitter records for dispatch, counts, borrowed methods, raw/listener inspection, symbols, prepend and direct _events mutation. Signal/IPC references are actual add/remove observers; the scheduler no longer mirrors listeners. IPC reference/fork tests pass. An attempted bound-once wrapper changed the correct Node26 ordinary wrapper shape; the new regression caught it and the original matching shape is restored. Initial failed compiler checks, signal counts and wrapper outputs are separate from corrected runs.

CLI --version/-v now derive the configured SDK version and respect eval/option/guest boundaries; the eight-case oracle passes. The frozen 400-case predecessor's instruction/load evidence is separate and does not qualify newer source. Diagnostic-report generation/file I/O and abort/default-warning/source/async contracts remain open. A recorded warning probe confirms that the default onWarning listener is missing and stderr printing survives listener removal. Total Node parity and a framework win remain unproven. No authored performance tuning accompanies these changes.
''');print(root)
