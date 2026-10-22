from pathlib import Path
import json,hashlib,subprocess,shutil

def package(prefix,dirname,before,after,cases,api_name):
 b=json.loads(Path('work/'+prefix+'-'+before+'.json').read_text());a=json.loads(Path('work/'+prefix+'-'+after+'.json').read_text());assert a==b and not b['status'];source=b['candidate_source']
 for p,h in b['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
 for suffix in [api_name,'framework','web','paired-callgrind']:
  assert Path('work/'+prefix+'-'+suffix+'.exit').read_text().strip()=='0',suffix
 assert f'{cases} passed; 0 failed' in Path('work/'+prefix+'-'+api_name+'.log').read_text()
 r=json.loads(Path('work/'+prefix+'-paired-callgrind/report.json').read_text());assert r['all_valid'] and r['identities_unchanged'] and len(r['results'])==32
 root=Path('evidence')/dirname;root.mkdir(exist_ok=True)
 for pattern in [prefix+'-*.log',prefix+'-*.exit',prefix+'-qualification-*.json']:
  for p in Path('work').glob(pattern):
   if p.is_file():shutil.copy2(p,root/p.name)
 for suffix in ['web','paired-callgrind']:
  shutil.copytree('work/'+prefix+'-'+suffix,root/suffix,dirs_exist_ok=True)
 if prefix=='sdk-emit-warning':
  shutil.copy2('work/node-stable-release-recheck-2026-10-16.json',root/'node-stable-release-recheck.json')
  for folder in ['sdk-emit-warning-upstream','sdk-emit-warning-upstream-correct-harness','warning-options-access-order']:
   shutil.copytree(Path('work')/folder,root/folder,dirs_exist_ok=True)
  for name in ['test-process-warning-sdk-authority.js','test-process-rejection-warning-arguments.js']:
   (root/('oracle-'+name)).write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/'+name]))
 assert not subprocess.check_output(['git','diff','a45551524',source,'--','crates/quench-runtime','crates/quench-regexp','Cargo.toml','Cargo.lock'])
 (root/'qualification.json').write_text(json.dumps(b|{'api_cases_passed':cases,'runtime_tests':672,'runtime_source_qualified_by':'a45551524','framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit']for c in r['comparisons']),'total_node_parity':False,'throughput_win':False,'source_only_build_matched_comparison':False},indent=2)+'\n')
 rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in r['comparisons'])
 (root/'README.md').write_text(f'''# Frozen SDK warning qualification

Source {source} passes {cases} serial Node v26.12.0 API comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Source/CLI/runner/test/oracle manifests match before and after the passing API run. Runtime/RegExp/Core Cargo inputs match the integrated 672-test passing a45551524 tree. Later source is excluded.

All 32 instruction captures are valid with unchanged identities. Frozen CLI hashes and source manifests identify the measured code even if the profiler records a later working-tree revision. The baseline is frozen 03ea582d2. Candidate iteration opt1/256 CGUs/no LTO/incremental=false, integrated shared-source differences and concurrent host activity prevent isolated source attribution, production timing or RSS claims.

| Fixture | Instruction delta | Guard |
| --- | ---: | --- |
{rows}

SDK warning listener policy removes default output when listeners are removed and preserves suppression/startup options. The newer sdk-emit-warning source also uses the actual SDK public emitWarning function, avoiding extra Proxy reads and preserving public source/identity/constructor policy; promise producers use ordinary public string/type or Error arguments. Its focused and original upstream fixtures are retained. Initial direct upstream-file runs treated CommonJS fixtures as ESM because of the surrounding repository package type; correct run-parallel --one runs pass. These setup failures are not parity findings. The newer qualification also retains its initial 407/408 API run (Quench SIGTERM handler did not return status7 under the existing 300ms deadline), unchanged focused pass, paired 20/20 ready-marker/handler probe and unchanged full repeat. The transient cause remains unproven; no deadline changed. The older warning-listener qualification separately retains its initial Node-oracle PID startup deadline failure and unchanged passing repeat; no deadlines changed.

Diagnostic report generation, native metrics, abort/async hooks, broader source/namespace behavior and total Node parity remain open. The latest short Express JSON load has 249 timeouts. There is no framework throughput win or authored performance optimization.
''')
 shutil.copy2(__file__,root/'package-evidence.py')
 print(root)

if __name__=='__main__':
 package('sdk-warning-policy','web-sdk-warning-listener-instruction-qualification-2026-10-16','qualification-before','qualification-after-repeat',406,'api-repeat')
 package('sdk-emit-warning','web-sdk-warning-construction-parity-2026-10-16','qualification-before-api','qualification-after-repeat',408,'api-repeat')
