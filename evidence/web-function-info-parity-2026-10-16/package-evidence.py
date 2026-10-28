from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-function-info-parity-2026-10-16');root.mkdir(exist_ok=True)
a=json.loads(Path('work/function-prototype-lookup-qualification-after-api.json').read_text());source=a['reconciled_candidate_source'];assert a['api_exit']==0 and all(a['identity_checks'].values())
for p,h in a['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
for name in ['runtime','final-workspace','api','framework','web','paired-callgrind']:assert Path('work/function-prototype-lookup-'+name+'.exit').read_text().strip()=='0',name
assert Path('work/function-prototype-lookup-layout-focused.exits').read_text().strip()=='0 0'
g=json.loads(Path('work/function-prototype-lookup-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
for pattern in ['function-prototype-lookup-*','function-prototype-info-*.stdout','function-prototype-info-*.stderr','function-prototype-info-*.exit','function-prototype-info-*.cjs','function-name-prototype-read-*','function-prototype-accessor-transition-*','function-prototype-lookup-core-oracle.*','function-constructor-body-layout-*','internal-key-namespace-actual-nul-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind']:shutil.copytree(Path('work/function-prototype-lookup-'+name),root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=376,runtime_tests=659,framework_fixtures=43,web_correctness_targets=9,instruction_limit_met=g['instruction_limit_met'],passed_instruction_guards=sum(c['within_limit']for c in g['comparisons']),warmup_control={'rows':80,'parent_differing_rows':21,'candidate_differing_rows':3,'newly_differing_rows':0},subsequent_function_constructor_source_layout_changes_excluded=True,total_node_parity=False,throughput_win=False)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n');(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff',a['base_source'],source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons']);v=[c['candidate_startup']-c['base_startup']for c in g['comparisons']]
(root/'README.md').write_text(f'''# Function public names, debug origins and native-info lookup

Frozen source {source} passes all 376 serial stable Node v26.12.0 API comparisons (264.88 seconds), 659 runtime tests, workspace/all-target checking, two added focused comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Source, oracle, CLI, runner and test identities remain unchanged through the full API run. Manifest file hashes reconcile to the source commit; the retained frozen CLI remains unchanged. Later host builds and uncommitted Function-constructor source-layout changes are excluded.

Dynamic constructors initialize anonymous as public creation data without marking a user name reconfiguration. Debug names derive from actual program origin; stack/detailed names and getter-free constructor display distinguish public anonymous from the empty source name across four constructor kinds. Descriptor mutations, throwing getters, retained CallSites and prototype roles are covered.

Prototype setup and named native-info lookup are explicit lifecycle states. Function name/length descriptor transitions share the immutable shape representation; compaction preserves them. Native accessor visibility derives from ordered descriptors, including deletion/restoration of length. The 64-row matrix matches both ordinary and jitless stable Node; the core regression passes in both execution modes. An initial wider 48-row probe exposes three deleted-length differences, then the descriptor-position repair resolves them. The first workspace check rejects duplicated match arms; corrected checks pass. A file-mode supplemental invocation is rejected by both runtimes because the repository's js files are ESM; the intended eval-mode comparison succeeds with empty stderr and exit zero.

A separate 80-row integrity/warm-up control still has three differing rows (readBefore/missing, own and computed). The parent frozen 74c has 21 differences on that same input, and no previously matching row becomes different. These are row comparisons, not distinct bug counts or full conformance gains. V8 lazy feedback allocation and prototype-fast lookup remain incompletely modeled; no guessed invocation threshold or fixture policy is added. An isolated GC probe does not support attributing the original difference to weak ownership.

All 32 matched fixed-work instruction captures are valid with unchanged frozen identities against 03ea582d2. Only two of eight 1% guards pass. These integrated deltas include shared changes since the baseline and do not isolate attribution to this repair. The new named lookup hook's placement requires investigation; no authored performance optimization or throughput/RSS claim is made. A subsequent source-layout build runs alongside part of instruction collection; wall times are not compared, and the measured executables are frozen separately.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. Guard failure is retained, not reported as a pass or benchmark win. The 20-case body-layout negative control, confirmed ordinary NUL-key visibility failure, warm-up transitions, raw UTF16 source, sourceURL/source maps, other stack methods and the broader Node audit remain open. Total Node parity and a valid winning web throughput result are not established.
''')
shutil.copy2('work/package-function-info-evidence.py',root/'package-evidence.py')
print(source,root,'guards',a['passed_instruction_guards'])
