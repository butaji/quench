from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-class-initializer-private-brands-parity-2026-10-16')
before=json.loads(Path('work/class-initializer-qualification-before.json').read_text());after=json.loads(Path('work/class-initializer-qualification-after-api.json').read_text())
assert before==after and not before['status']
source=before['candidate_source']
for p,h in before['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for k in ['cli','node']:
 b=before['binaries'][k];assert hashlib.sha256(Path(b['path']).read_bytes()).hexdigest()==b['sha256'],k
for name in ['runtime','workspace','api','framework','web','paired-callgrind']:
 assert Path('work/class-initializer-context-'+name+'.exit').read_text().strip()=='0',name
report=json.loads(Path('work/class-initializer-context-paired-callgrind/report.json').read_text());assert report['all_valid'] and report['identities_unchanged'] and len(report['results'])==32
root.mkdir(exist_ok=True)
for pattern in ['class-initializer-*','private-brand-initialization-*','private-brand-static-context-*','private-brand-declaration-*','static-block-context-*','static-block-arguments-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind']:
 shutil.copytree(Path('work/class-initializer-context-'+name),root/name,dirs_exist_ok=True)
for name in ['private-brand-initialization','static-block-context']:
 shutil.copy2('crates/quench-node-test/node-tests/test-'+name+'.js',root/('oracle-'+name+'.js'))
shutil.copy2(__file__,root/'package-evidence.py')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','6ded4eada',source,'--','crates','docs']))
summary=before|{'api_cases_passed':381,'runtime_tests':667,'framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit']for c in report['comparisons']),'instruction_limit_met':report['instruction_limit_met'],'total_node_parity':False,'throughput_win':False,'candidate_build':{'profile':'iteration','opt_level':1,'codegen_units':256,'lto':False,'incremental':False},'source_only_build_matched_comparison':False}
(root/'qualification.json').write_text(json.dumps(summary,indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass'if c['within_limit']else'FAIL'} |"for c in report['comparisons'])
(root/'README.md').write_text(f'''# Private brands follow initialization; static blocks retain their context

Frozen source {source} passes 381 serial Node v26.12.0 API comparisons, 667 runtime tests, workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes match before and after API verification. These results qualify this exact source and exclude later changes.

Node's class prototypes have no private brands merely from declarations. Static data fields remain absent until initialized, while static private methods/accessors precede field execution. The retained old probe incorrectly reports prototype membership and early static-data membership true. Corrected comparisons cover initialized receivers, field order, getter/setter pairs, stamping the class's own prototype, duplicate stamping and old values. Generic/specialized execution and binary round-trip verify these transitions.

Static blocks now retain their actual super home and execute in strict mode. Node comparisons cover inherited getters/setters with the actual class receiver, arrows, invalid ordinary-function super, undefined new.target, strict unresolved/readonly assignments and separate var scopes. The shared class-initializer context also prohibits lexical arguments in fields, static blocks and their arrows, while ordinary nested functions own arguments. The flag's existing serialized representation is retained in format 93.

An initial host build fails after erroneous deletion of an active incremental cache; it has no verification verdict. A later runtime run passes 666 cases and fails the existing auto-accessor regression: the hidden backing field depended on a prototype declaration brand for home discovery. Backing identities now retain their explicit lexical declaring-home binding, including source-invisible public instance auto-accessors. The complete 667-test retry passes. Raw failures remain distinct from successful checks; the early host retry predates the static-block edits and is not their proof.

All 32 matched instruction captures are valid with unchanged identities against 03ea582d2. The candidate uses the iteration profile (opt-level 1, 256 codegen units, no LTO) with incremental compilation disabled after cache/disk failures. Earlier snapshots used incremental compilation, so comparison includes build-configuration differences; the lower deltas cannot be attributed to this semantic repair. These integrated frozen-executable deltas do not establish production throughput/RSS. No authored optimization, total Node parity or web win is claimed. Ordinary NUL string-key visibility, broader private/stack/eval behavior and the wider Node backlog remain open.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}
''')
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()for p in root.rglob('*')if p.is_file()and p.name!='sha256.json'}
(root/'sha256.json').write_text(json.dumps(manifest,indent=2)+'\n');print(root)
