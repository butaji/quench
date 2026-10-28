from pathlib import Path
import json,subprocess,shutil,hashlib
root=Path('evidence/web-fused-field-source-parity-2026-10-16')
a=json.loads(Path('work/fused-field-source-qualification-after-api.json').read_text())
assert a['api_exit_code']==0 and all(a['identity_checks'].values())
source=subprocess.check_output(['git','rev-parse',a['reconciled_candidate_source']],text=True).strip()
for name in ['fused-field-source-published-workspace','fused-field-source-published-runtime-retry','fused-field-source-expanded-focused','fused-field-source-binary-focused','fused-field-source-framework','fused-field-source-web','fused-field-source-api','fused-field-source-paired-callgrind']:
 assert Path('work/'+name+'.exit').read_text().strip()=='0',name
g=json.loads(Path('work/fused-field-source-paired-callgrind/report.json').read_text())
assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
root.mkdir(exist_ok=True)
for pattern in ['fused-field-source-*.log','fused-field-source-*.exit','fused-field-source-qualification-*.json','fused-field-source-source.patch','fused-field-position-probe.cjs','fused-field-position-*.stdout','fused-field-position-*.stderr','fused-field-position-*.exit','fused-field-sink-probe.cjs','fused-field-sink-*.stdout','fused-binary-roundtrip-oracle.cjs']:
 for p in Path('work').glob(pattern): shutil.copy2(p,root/p.name)
for name in ['fused-field-source-web','fused-field-source-paired-callgrind']:
 shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=363,runtime_tests=654,framework_fixtures=43,web_correctness_targets=9,instruction_limit_met=g['instruction_limit_met'],subsequent_eval_origin_changes_excluded=True)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff',a['base_source'],source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |" for c in g['comparisons'])
v=[c['candidate_startup']-c['base_startup'] for c in g['comparisons']]
(root/'README.md').write_text(f'''# Source locations within fused field operations

Frozen source {source} passes 363/363 serial comparisons against stable Node v26.12.0 (exit zero, 229.38 seconds), 654 runtime tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes remain fixed during the full API run. The base source, patch and added fixture hash reconcile to this commit. Later eval-origin work is excluded.

The retained negative control shows this.a.b reports b's source position for both getters. Field sites now retain each read/read/write coordinate. Scoped active-component state selects the executing operation and restores the previous context after success or failure. General and specialized execution, decoded binaries, setters, throwing getters and subsequent captures are checked. Bytecode format 91 serializes the coordinates; validation rejects zero coordinates and the reader rejects invalid option tags. The sink probe reproduced the first-read error; its setter position already matched before this repair.

The first complete runtime build fails with ENOSPC before tests. Its log is retained separately from the successful 654-test retry. Completed compiler caches were removed; binaries, SDKs and evidence were preserved.

All 32 fresh matched instruction captures are valid with unchanged frozen binary identities. Measurements compare against frozen 03ea582d2, use startup subtraction and exact completed fixture counts, and cover all eight fixtures. They are integrated comparisons, not isolated attribution, throughput, RSS or a benchmark win.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. Total Node parity and web benchmark dominance remain unproven. Eval/sourceURL/source maps, the complete CallSite surface, internal string-key visibility and broader Node APIs remain open.
''')
shutil.copy2('work/package-fused-field-source-evidence.py',root/'package-evidence.py')
print(source,root,g['instruction_limit_met'])
