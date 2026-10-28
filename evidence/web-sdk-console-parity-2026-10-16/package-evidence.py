from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-sdk-console-parity-2026-10-16')
prefix='console-sdk-current-process'
b=json.loads(Path(f'work/{prefix}-qualification-before-api.json').read_text());a=json.loads(Path(f'work/{prefix}-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for name in ['api','framework','web','workspace','runtime','instructions']:
 assert Path(f'work/{prefix}-{name}.exit').read_text().strip()=='0',name
assert '425 passed; 0 failed' in Path(f'work/{prefix}-api.log').read_text()
assert '672 passed; 0 failed' in Path(f'work/{prefix}-runtime.log').read_text()
r=json.loads(Path(f'work/{prefix}-instructions/report.json').read_text());assert r['all_valid'] and r['identities_unchanged'] and len(r['results'])==32
assert len(r['comparisons'])==8 and all(c['within_limit']for c in r['comparisons'])
root.mkdir(exist_ok=True)
for pattern in [prefix+'-*.log',prefix+'-*.exit',prefix+'-qualification-*.json','console-sdk-authority-api.*','console-sdk-protected-api.*','console-sdk-authority-raw-binding-native-api.*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for folder,target in [(f'{prefix}-instructions','paired-callgrind'),('web-console-sdk-current-process-verify','web')]:shutil.copytree(Path('work')/folder,root/target,dirs_exist_ok=True)
shutil.copy2('work/node-stable-release-recheck-2026-10-16.json',root/'node-stable-release-recheck.json')
(root/'oracle-test-console-sdk-authority.js').write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/test-console-sdk-authority.js']))
(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':425,'runtime_tests_passed':672,'framework_cases_passed':43,'web_correctness_targets_passed':9,'instruction_guards_passed':8,'total_node_parity':False,'throughput_win':False,'later_heap_census_work_excluded':True},indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | pass |"for c in r['comparisons'])
(root/'README.md').write_text(f'''# Original SDK Console qualification

Frozen source `{source}` passes 425 serial Node v26.12.0 API comparisons (252.16s), 672 Core tests (47.24s), 43 framework fixtures and nine no-JIT web correctness targets. Source, CLI, runner, test binary and Node manifests match before/after API qualification. Later heap-census source changes are excluded.

The actual configured SDK owns Console constructor and output policy, global namespace and diagnostics. Duplicate Console/timer policy is removed. SDK SafeMap/SafeWeakMap protect diagnostics storage from public prototype mutation. The original SDK test-binding bridge exposes protected primordials and cached raw native bindings while observing current process warning state.

Retained failures: the initial Console run passed420/421 because diagnostics used public WeakMap.get; protected storage fixes that. The next run passed417/421 because the original SDK test bridge received library adapters rather than raw native objects. Raw-provider correction passed118/120 native cases; remaining warning differences came from startup-intrinsics restoration masking current process state. The final correction passes124/124 native cases and425/425 full API cases without changed deadlines.

All32 fixed-work instruction captures are valid and identities unchanged; all8 guards pass against frozen03ea582d2. Candidate uses iteration opt1/256CGUs/noLTO/incremental=false; shared-source and build differences prevent isolated source or production speed/RSS attribution. Later heap-census compilation ran concurrently with instruction capture; wall times are not compared.

| Fixture | Instruction delta | Guard |
| --- | ---: | --- |
{rows}

These subsets do not establish total Node parity. The prior short Express JSON load reported249timeouts; no framework throughput victory or authored optimization is claimed.
''')
shutil.copy2(__file__,root/'package-evidence.py');print(root)
