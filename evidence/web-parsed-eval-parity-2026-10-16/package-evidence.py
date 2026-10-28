from pathlib import Path
import json,subprocess,shutil,hashlib
root=Path('evidence/web-parsed-eval-parity-2026-10-16')
a=json.loads(Path('work/compiled-eval-qualification-after-api.json').read_text())
assert a['api_exit_code']==0 and all(a['identity_checks'].values())
source=a['reconciled_candidate_source']
for name in ['compiled-eval-workspace-cleanup','compiled-eval-runtime','compiled-eval-expanded-focused','compiled-eval-source-matrices','compiled-eval-framework','compiled-eval-web','compiled-eval-api','compiled-eval-paired-callgrind']:
 assert Path('work/'+name+'.exit').read_text().strip()=='0',name
g=json.loads(Path('work/compiled-eval-paired-callgrind/report.json').read_text())
assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
root.mkdir(exist_ok=True)
for pattern in ['compiled-eval-*.log','compiled-eval-*.exit','compiled-eval-qualification-*.json','compiled-eval-source.patch','eval-syntax-policy*','eval-callsite-identity*','compiled-eval-syntax-policy*','compiled-eval-callsite-identity*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['compiled-eval-web','compiled-eval-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=365,runtime_tests=654,framework_fixtures=43,web_correctness_targets=9,instruction_limit_met=g['instruction_limit_met'],subsequent_source_excerpt_and_eval_origin_changes_excluded=True)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff',a['base_source'],source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons'])
v=[c['candidate_startup']-c['base_startup']for c in g['comparisons']]
(root/'README.md').write_text(f'''# Eval executes parsed programs with original source positions

Frozen source {source} passes 365/365 serial comparisons against stable Node v26.12.0 (exit zero, 229.56 seconds), 654 runtime tests, workspace/all-target checking, an expanded eval oracle matrix, seven earlier source/stack matrices, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes remain unchanged through the full API run. The source patch and added fixture reconcile to this commit; its publication base differs from its compilation base only by evidence. Later source-excerpt and eval-origin edits are excluded.

The retained negative controls show valid eval string "for(;false;)" throws SyntaxError and hashbang Math.max(2,3) returns undefined where Node returns 3. Direct/indirect eval now execute the existing OXC Eval goal, and host evaluation uses the Script goal, preserving raw source and statement completion. More than 1,100 obsolete lines are removed: the manual text interpreter, expression-return wrappers, redundant grammar helpers, fallback error flag and wrapper-only binding state. The Function-constructor compiler retains unconditional parameter-boundary validation.

OXC owns regexp literal spans and flags; the original UTF16 pattern units remain authoritative, including lone surrogates and parentheses. Expanded oracle comparisons cover completion, strictness, syntax errors, hashbangs, caller bindings, arguments, new.target, original call positions, host Script expressions and regexp units. The original probes now show the correct values and eval columns 1/3:1 instead of wrapper column 22. Their complete raw stacks still expose the separately open filename/origin/host-frame gaps.

Initial borrowed-slice and obsolete-dead-code check failures are retained separately from successful checks. Completed compiler caches are removed as needed; binaries, SDKs and measurement evidence are preserved.

All 32 fresh strict matched captures are valid with unchanged frozen binary identities against 03ea582d2. Startup subtraction uses each binary's N=0 capture; regular-file outputs must complete the exact fixture count. These are integrated instruction measurements, not isolated attribution, throughput, RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. No authored optimization, total Node parity or web benchmark win is claimed. Complete eval-origin/sourceURL/source-map metadata, Function-constructor source positions, fatal excerpts, internal-key visibility and broader Node APIs remain open in this frozen snapshot.
''')
shutil.copy2('work/package-compiled-eval-evidence.py',root/'package-evidence.py')
print(source,root,g['instruction_limit_met'])
