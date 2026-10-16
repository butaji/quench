from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-private-field-identity-parity-2026-10-16');root.mkdir(exist_ok=True)
a=json.loads(Path('work/private-field-storage-qualification-after-api.json').read_text());source=a['reconciled_candidate_source'];assert a['api_exit']==0 and all(a['identity_checks'].values())
for p,h in a['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
for name in ['runtime','workspace','api','framework','web','paired-callgrind','integrated-focused']:assert Path('work/private-field-storage-'+name+'.exit').read_text().strip()=='0',name
g=json.loads(Path('work/private-field-storage-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
for pattern in ['private-field-storage-*','private-field-identity-*.stdout','private-field-identity-*.stderr','private-field-identity-*.exit','test-private-field-identity-*.stdout','test-private-field-identity-*.stderr','private-field-eval-home-*','private-name-repeated-class-stamping-*','private-eval-environments-*.stdout','private-eval-environments-*.stderr','private-eval-environments-probe.cjs','internal-key-namespace-actual-nul-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind']:shutil.copytree(Path('work/private-field-storage-'+name),root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=378,runtime_tests=663,framework_fixtures=43,web_correctness_targets=9,focused_api_comparisons=5,instruction_limit_met=g['instruction_limit_met'],passed_instruction_guards=sum(c['within_limit']for c in g['comparisons']),subsequent_lexical_private_binding_changes_excluded=True,total_node_parity=False,throughput_win=False)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n');(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff',a['base_source'],source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons']);v=[c['candidate_startup']-c['base_startup']for c in g['comparisons']]
(root/'README.md').write_text(f'''# Private data belongs to its declaring home and receiver

Frozen source {source} passes all 378 serial stable Node v26.12.0 API comparisons (267.64 seconds), 663 runtime tests (46.04 seconds), workspace/all-target checking, five focused private-related comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test identities remain fixed through the API run; source-file hashes reconcile to the commit. Later lexical private-binding changes, including format 93, are excluded from this format-92 proof.

The original repeated-class probe reads [2,2] where Node reads [1,2]. A receiver now owns one traced entry for each actual (declaring home, logical name), containing both its brand and optional data. Values are no longer public string properties sharing one logical atom. An atomic private-data definition checks duplicates before mutation, preserves the old value after a failed second stamp and ignores public extensibility and Proxy traps. Private method brands likewise do not consult public extensibility. The new operation and its operands are serialized in bytecode format 92; throwing brand operations declare that effect.

The full guest comparison covers ordinary objects, arrays, functions, null prototypes, frozen/sealed/nonextensible targets, Proxy/revoked Proxy, duplicate error text and old values, frozen methods/accessors, static fields and cloning. The first storage repair leaves nine rows differing only in direct eval. Eval-capable methods now register their actual synthetic home capture in dynamic-name metadata, so eval does not infer private context from receiver prototypes. The complete stated matrix passes. Root tests retain two private values while the receiver lives and release both after dropping it; both execution modes and original/decoded execution pass.

A separate mixed private-eval probe remains failing: instance-method eval of static names, ordinary nested-function private eval and static-method eval of instance names differ from Node. Two same-home controls pass. This checkpoint does not claim complete lexical-private environments, ordinary NUL string-key namespaces or all class behavior. Those raw failures remain separate from the passing matrix.

All 32 strict matched instruction captures against 03ea582d2 are valid with frozen identities unchanged, but only two of eight 1% guards pass. These integrated deltas do not isolate attribution and are not throughput, RSS or benchmark dominance. A later private-binding type check runs alongside part of collection; wall times are not compared and the executables are independently frozen.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. Six guard failures, named-lookup hook placement, three prototype warm-up differences, the ordinary NUL-key failure, broader private eval/source metadata and the full Node audit remain open. No authored performance optimization, total parity or winning web throughput result is claimed.
''')
shutil.copy2('work/package-private-field-evidence.py',root/'package-evidence.py');print(source,root,'guards',a['passed_instruction_guards'])
