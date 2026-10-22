from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-private-lexical-eval-parity-2026-10-16')
before=json.loads(Path('work/private-lexical-qualification-before.json').read_text())
after=json.loads(Path('work/private-lexical-qualification-after-api.json').read_text())
assert before==after,'source/oracle/binary identity changed'
assert not before['status'],'candidate working tree was not clean'
source=before['candidate_source']
for p,h in before['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for k in ['cli','node']:
 b=before['binaries'][k]
 assert hashlib.sha256(Path(b['path']).read_bytes()).hexdigest()==b['sha256'],k
for name in ['runtime','api','workspace','framework','web','paired-callgrind']:
 assert Path('work/private-lexical-bindings-'+name+'.exit').read_text().strip()=='0',name
report=json.loads(Path('work/private-lexical-bindings-paired-callgrind/report.json').read_text())
assert report['all_valid'] and report['identities_unchanged'] and len(report['results'])==32
root.mkdir(exist_ok=True)
for pattern in ['private-lexical-*','private-eval-environments-*','private-ordinary-eval-*','private-static-block-eval-*','private-brand-declaration-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind']:
 shutil.copytree(Path('work/private-lexical-bindings-'+name),root/name,dirs_exist_ok=True)
shutil.copy2('crates/quench-node-test/node-tests/test-private-eval-environments.js',root/'oracle-fixture.js')
shutil.copy2(__file__,root/'package-evidence.py')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','4224de82a',source,'--','crates','docs']))
summary=before|{'api_cases_passed':379,'runtime_tests':665,'framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit']for c in report['comparisons']),'instruction_limit_met':report['instruction_limit_met'],'total_node_parity':False,'throughput_win':False}
(root/'qualification.json').write_text(json.dumps(summary,indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass'if c['within_limit']else'FAIL'} |"for c in report['comparisons'])
(root/'README.md').write_text(f'''# Lexical private eval environments

Frozen source {source} passes 379 serial Node v26.12.0 API comparisons, 665 runtime tests, workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes match before and after API verification. These results qualify this exact source, excluding later merges or repairs.

Private names now retain lexical bindings to each actual declaring home, separately from the method's super home. Mixed static/instance names, escaped ordinary closures, nearest class shadowing and outer private names are covered by the Node comparison. Generic/specialized execution and bytecode round-trip cover the added binding kind in format 93. Ordinary-function eval uses OXC's function grammar inside the lexical private-name parsing context; invalid super access, unknown private names and top-level return remain errors.

A retained malformed-eval probe previously returns 9 where Node throws SyntaxError. Parsed eval bodies must now match their generated source span, preventing user text from replacing the context wrapper's extracted body. The corrected Node comparison passes. Earlier type-check failures and unbounded probe outputs remain diagnostic records, not successful qualifications.

Separate probes still differ for super inside static-block eval, prototype private membership and static data membership before initialization. Ordinary NUL string-key visibility, broader private environment/brand behavior and the full Node backlog remain open. This is not total Node parity or a web throughput win. No authored performance optimization is claimed.

All 32 matched instruction captures are valid with unchanged identities against 03ea582d2. These integrated deltas do not isolate attribution or establish production throughput/RSS.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}
''')
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()for p in root.rglob('*')if p.is_file()and p.name!='sha256.json'}
(root/'sha256.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(root)
