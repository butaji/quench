from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-function-source-layout-parity-2026-10-16');root.mkdir(exist_ok=True)
a=json.loads(Path('work/function-constructor-source-layout-qualification-after-api.json').read_text());source=a['reconciled_candidate_source'];assert a['api_exit']==0 and all(a['identity_checks'].values())
for p,h in a['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
for name in ['integrated-runtime','integrated-workspace','api','framework','web','paired-callgrind']:assert Path('work/function-constructor-source-layout-'+name+'.exit').read_text().strip()=='0',name
assert Path('work/function-constructor-source-layout-integrated-focused.exits').read_text().strip()=='0 0'
g=json.loads(Path('work/function-constructor-source-layout-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
for pattern in ['function-constructor-source-layout-*','function-constructor-canonical-core-*','function-constructor-body-layout-*','function-info-merge-test-preservation.json','function-info-publication-*.log','function-info-publication-*.exit','function-info-merged-workspace.*','dynamic-function-source-layout-plan.md']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind']:shutil.copytree(Path('work/function-constructor-source-layout-'+name),root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=377,runtime_tests=661,framework_fixtures=43,web_correctness_targets=9,source_layout_oracle_rows=76,instruction_limit_met=g['instruction_limit_met'],passed_instruction_guards=sum(c['within_limit']for c in g['comparisons']),subsequent_private_field_storage_changes_excluded=True,total_node_parity=False,throughput_win=False)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n');(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff',a['base_source'],source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons']);v=[c['candidate_startup']-c['base_startup']for c in g['comparisons']]
(root/'README.md').write_text(f'''# Canonical dynamic-Function source and scope

Frozen source {source} includes shared prepared-policy merge b7723be8e and passes 377/377 serial stable Node v26.12.0 API comparisons (267.03 seconds), 661 runtime tests (46.50 seconds), workspace/all-target checking, 76 source-layout oracle rows, the earlier native-info controls, 43 framework fixtures and nine no-JIT web correctness targets. Source, oracle, CLI, runner and test executable identities remain fixed through the API run. Source-file hashes reconcile to the commit. Subsequent private-field storage/bytecode changes are excluded.

The retained 20-case negative control differs in every row: Function body trimming loses whitespace and the compiler wrapper places body expressions on the header line. A single kind-owned canonical renderer now supplies compilation and public toString. Whitespace and all JavaScript line separators survive; parameters, body captures, nested closures and eval origins match the 76-row oracle across four kinds. OXC validates the generated name and parameter boundary, then only the synthetic outer identifier is removed before semantic binding: anonymous is not a self binding and actual parameter/nested names remain intact.

The first core parameter-column expectation (24) fails on Node itself; the actual result is 25. V8 counts its hidden opening parenthesis in first-line parameter positions. The corrected control and compiler retain that wrapper directly, without a compensating source-column offset. The initial failed oracle and corrected success remain separate artifacts.

Pre-merge source passes 660 runtime tests and the 76-row comparison. A fresh sync consumes 0198edb75 shared prepared-policy code and changes the integration boundary. Both parents' test chunks survive; one moved glob test is deduplicated and one external blank line differs. No guest source/assertion is discarded. Merged workspace checks pass, then the restored source-layout repair is rebuilt and requalified with all 661 runtime tests and 377 APIs. Parent proof is not reused as integration proof. The preservation receipt counts column-zero chunks; the actual Rust runner count is authoritative.

All 32 matched instruction captures against 03ea582d2 are valid with frozen identities unchanged. Only two of eight 1% guards pass. These are integrated deltas, not isolated attribution, throughput, RSS or a benchmark win. A next-stage private-field type check/build runs alongside part of instruction collection; wall times are not compared and measurement uses independently frozen executables.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. The named native-info hook's placement remains an instruction-cost finding; no authored performance optimization has begun. Three prototype feedback/warm-up differences, ordinary NUL-key visibility, repeated-class private-field identity, raw UTF16 and normalized-parameter source metadata, sourceURL/source maps, missing stack methods and the broader Node conformance audit remain open. Total Node parity and a winning web throughput result are not established.
''')
shutil.copy2('work/package-function-source-layout-evidence.py',root/'package-evidence.py');print(source,root,'guards',a['passed_instruction_guards'])
