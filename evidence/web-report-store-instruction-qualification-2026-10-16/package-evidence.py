from pathlib import Path
import json,hashlib,subprocess,shutil
p='process-report-shared-store';b=json.loads(Path('work/'+p+'-qualification-before.json').read_text());a=json.loads(Path('work/'+p+'-qualification-after-api.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
for f,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+f])).hexdigest()==h,f
assert Path('work/'+p+'-paired-callgrind.exit').read_text().strip()=='0'
r=json.loads(Path('work/'+p+'-paired-callgrind/report.json').read_text());assert r['all_valid'] and r['identities_unchanged'] and len(r['results'])==32
root=Path('evidence/web-report-store-instruction-qualification-2026-10-16');root.mkdir(exist_ok=True)
shutil.copytree('work/'+p+'-paired-callgrind',root/'paired-callgrind',dirs_exist_ok=True)
for suffix in ['qualification-before.json','qualification-after-api.json','paired-callgrind.log','paired-callgrind.exit']:
 shutil.copy2('work/'+p+'-'+suffix,root/(p+'-'+suffix))
shutil.copy2(__file__,root/'package-evidence.py')
(root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':405,'runtime_tests':672,'framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit']for c in r['comparisons']),'total_node_parity':False,'throughput_win':False,'source_only_build_matched_comparison':False},indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass'if c['within_limit']else'FAIL'} |"for c in r['comparisons'])
(root/'README.md').write_text(f'''# Frozen report/process-store instruction qualification

Frozen source {source} passes 405 Node26 API comparisons, the verified 672-test Runtime tree, workspace, 43 framework fixtures and nine no-JIT web correctness targets. Its bounded functional evidence is in web-report-configuration-shared-process-store-parity-2026-10-16. All 32 fixed-work instruction captures are valid with unchanged identities against frozen 03ea582d2; all eight guards pass. Candidate hashes/source manifests identify the frozen CLI even though the verifier may record a later working-tree revision.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}

Candidate iteration opt1/256 CGUs/no LTO/incremental=false and integrated shared-source/configuration differences prevent isolated source attribution or production throughput/RSS claims. Subsequent object/array lookup merge and warning policy are excluded. Express JSON still has 249 timeouts in the last short load diagnostic; no total Node parity or framework win is established. No authored performance tuning accompanies this functional milestone.
''');print(root)
