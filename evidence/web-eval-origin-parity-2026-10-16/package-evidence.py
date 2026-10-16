from pathlib import Path
import json,subprocess,shutil,hashlib
root=Path('evidence/web-eval-origin-parity-2026-10-16')
a=json.loads(Path('work/eval-origin-qualification-after-api.json').read_text())
assert a['api_exit_code']==0 and all(a['identity_checks'].values())
source=a['reconciled_candidate_source']
for name in ['eval-origin-workspace','eval-origin-runtime','eval-origin-focused','eval-origin-framework','eval-origin-web','eval-origin-api','eval-origin-paired-callgrind']:
 assert Path('work/'+name+'.exit').read_text().strip()=='0',name
g=json.loads(Path('work/eval-origin-paired-callgrind/report.json').read_text())
assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
root.mkdir(exist_ok=True)
for pattern in ['eval-origin-*.log','eval-origin-*.exit','eval-origin-qualification-*.json','eval-origin-source.patch','callsite-eval-classification*','eval-callsite-identity*','callsite-prototype-surface*','callsite-location-methods*','eval-syntax-policy*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['eval-origin-web','eval-origin-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=364,runtime_tests=654,framework_fixtures=43,web_correctness_targets=9,instruction_limit_met=g['instruction_limit_met'],subsequent_parsed_eval_changes_excluded=True)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff',a['base_source'],source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons'])
v=[c['candidate_startup']-c['base_startup']for c in g['comparisons']]
(root/'README.md').write_text(f'''# Eval compilation origin

Frozen source {source} passes 364/364 serial stable Node v26.12.0 API comparisons (exit zero, 228.84 seconds), 654 runtime tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes stay fixed through the full API run. The source patch and fixture hashes reconcile before committing identical code. Later parsed-eval changes are excluded.

The retained negative control shows direct eval, indirect eval and Function-constructor frames report isEval=false where Node reports true. Stored compilation origin is independent of binding scope. Captured CallSites copy that classification. Expanded comparisons cover escaped and strict closures, all four Function-constructor kinds, ordinary functions called from eval, host scripts with eval-like filenames and retained CallSites after garbage collection. The repaired negative control matches Node.

All 32 fresh strict matched instruction captures are valid with unchanged identities against frozen 03ea582d2. Startup is subtracted using each binary's own N=0 capture and workloads must finish the exact fixture count. These integrated measurements do not isolate attribution or establish production throughput, RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. No authored performance optimization, total Node parity or benchmark win is claimed.

Retained follow-up probes show incomplete CallSite prototype methods, eval-origin descriptions, synthetic filenames and expression-wrapper positions. A valid eval string containing for(;false;) incorrectly throws, and a hashbang Math.max(2,3) script returns undefined instead of 3. Replacing the manual evaluator is subsequent work. SourceURL/source maps, native frame identities, internal-key visibility and broader Node APIs remain open.
''')
shutil.copy2('work/package-eval-origin-evidence.py',root/'package-evidence.py')
print(source,root,g['instruction_limit_met'])
