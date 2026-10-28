from pathlib import Path
import json,hashlib,shutil,subprocess
prefix='process-uncaught-monitor'
b=json.loads(Path(f'work/{prefix}-qualification-before.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','workspace','framework','web','focused-events','focused-capture','paired-callgrind']:assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0',name
assert '396 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text()
assert not subprocess.check_output(['git','diff','f4e6ed305',source,'--','crates/quench-runtime','crates/quench-regexp','Cargo.toml','Cargo.lock'])
r=json.loads(Path('work/'+prefix+'-paired-callgrind/report.json').read_text());assert r['all_valid'] and r['identities_unchanged'] and len(r['results'])==32
root=Path('evidence/web-uncaught-monitor-order-parity-2026-10-16');root.mkdir(exist_ok=True)
for family in [prefix+'-*','process-monitor-capture-transition-*','process-uncaught-emit-truthiness-*','process-capture-getter-order-*','process-capture-monitor-order-probe*']:
 for p in Path('work').glob(family):
  if p.is_file():shutil.copy2(p,root/p.name)
for folder in ['web','paired-callgrind']:shutil.copytree(Path('work')/(prefix+'-'+folder),root/folder,dirs_exist_ok=True)
for name in ['process-monitor-capture-transition','process-uncaught-emit-truthiness','process-capture-getter-order']:
 (root/('oracle-'+name+'.js')).write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/test-'+name+'.js']))
shutil.copy2(__file__,root/'package-evidence.py')
(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':396,'api_seconds':225.21,'runtime_tests':671,'runtime_tree_verified_from':'f4e6ed305','framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit'] for c in r['comparisons']),'total_node_parity':False,'throughput_win':False,'source_only_build_matched_comparison':False},indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in r['comparisons'])
(root/'README.md').write_text(f'''# Uncaught monitor ordering and shared process event dispatch

Frozen source {source} passes 396 serial Node v26.12.0 API comparisons (225.21s), workspace/all-target, five focused exception comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Before/after manifests are identical. Runtime and RegExp source plus Core Cargo inputs match the 671-test passing f4e6ed305 tree; later source is excluded.

Uncaught delivery emits uncaughtExceptionMonitor before selecting the primary capture slot. The monitor may replace that slot. Delivery reads captureFn again for its actual property call, matching the original SDK accessor order. A shared process-event call owner replaces duplicated dispatch; monitor results are ignored, whereas uncaught results use standard truthiness. Permanent oracle cases cover primary/replacement/fallback transitions, ignored Symbol returns, truthy string results and a getter selecting different callbacks for its first and second reads. Initial failing outputs are retained.

All 32 fixed-work captures are valid with unchanged identities against frozen 03ea582d2. Candidate is iteration opt1/256 CGUs/no LTO/incremental=false, with integrated regex/link-order changes. Earlier baseline builds used different configuration; these frozen-executable deltas do not isolate source attribution or qualify production throughput/RSS. The verifier may record a later working-tree revision while profiling; the frozen CLI hash and source manifest establish candidate provenance.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}

Auxiliary capture, report flags, abort toggles, async hooks and exact fatal lifecycle/source fidelity remain open. Total Node parity and a benchmark win are not established; no authored performance optimization accompanies these correctness fixes.
''')
print(root)
