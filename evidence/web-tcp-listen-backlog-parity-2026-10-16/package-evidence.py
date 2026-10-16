from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-tcp-listen-backlog-parity-2026-10-16')
before=json.loads(Path('work/tcp-listen-backlog-qualification-before.json').read_text());after=json.loads(Path('work/tcp-listen-backlog-qualification-after-api.json').read_text());assert before==after and not before['status'];source=before['candidate_source']
for p,h in before['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for k in ['cli','node']:
 b=before['binaries'][k];assert hashlib.sha256(Path(b['path']).read_bytes()).hexdigest()==b['sha256'],k
for name in ['api','workspace','framework','web','paired-callgrind']:assert Path('work/tcp-listen-backlog-'+name+'.exit').read_text().strip()=='0',name
assert Path('work/class-initializer-context-merged-runtime.exit').read_text().strip()=='0'
assert not subprocess.check_output(['git','diff','a01c1fb08',source,'--','crates/quench-runtime','Cargo.toml','Cargo.lock'])
r=json.loads(Path('work/tcp-listen-backlog-paired-callgrind/report.json').read_text());assert r['all_valid']and r['identities_unchanged']and len(r['results'])==32
load=json.loads(Path('work/tcp-listen-backlog-load-diagnostic/report.json').read_text());assert not load['complete']
root.mkdir(exist_ok=True)
for pattern in ['tcp-listen-backlog-*','tcp-default-listen-backlog-*','tcp-option-backlog-*','tcp-backlog-coercion-*','tcp-listen-lifecycle-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind','load-diagnostic']:shutil.copytree(Path('work/tcp-listen-backlog-'+name),root/name,dirs_exist_ok=True)
for name in ['runtime.log','runtime.exit','workspace.log','workspace.exit']:
 p=Path('work/class-initializer-context-merged-'+name);shutil.copy2(p,root/p.name)
for name in ['default','option']:shutil.copy2('crates/quench-node-test/node-tests/test-tcp-'+name+'-listen-backlog.js',root/('oracle-'+name+'-backlog.js'))
shutil.copy2(__file__,root/'package-evidence.py')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','a01c1fb08',source,'--','crates','docs']))
summary=before|{'api_cases_passed':383,'api_seconds':260.30,'runtime_tests':668,'runtime_tree_verified_from':'a01c1fb08','runtime_seconds':91.78,'framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit']for c in r['comparisons']),'total_node_parity':False,'throughput_win':False,'candidate_build':{'profile':'iteration','opt_level':1,'codegen_units':256,'lto':False,'incremental':False},'source_only_build_matched_comparison':False,'later_native_lifecycle_changes_excluded':True}
(root/'qualification.json').write_text(json.dumps(summary,indent=2)+'\n');rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass'if c['within_limit']else'FAIL'} |"for c in r['comparisons'])
(root/'README.md').write_text(f'''# TCP default and option backlog parity

Frozen source {source} passes 383 serial stable Node v26.12.0 API comparisons (260.30s), workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes remain unchanged across APIs. The runtime crate and workspace manifest/lock match a01c1fb08, whose 668 runtime tests pass in 91.78s; the later native lifecycle/constructor changes are excluded.

Node net.js uses 511 when no truthy backlog is supplied. System-call traces previously show Quench ordinary IPv4/default/HTTP listeners use 128, while only reusePort uses 511. A shared native socket-creation path now uses the Node default for all paths and preserves Unix/libuv address-reuse and supported reusePort behavior. Supplied options.backlog applies the JavaScript truthy default before shared ToNumber/ToInt32 conversion. Actual syscall comparisons match Node for four default cases and twenty net/HTTP option cases: numeric/string zero, seven, NaN, infinity, negative, wrapped integers and booleans. The fixture reports unavailable tracing explicitly. ss reports zero queue capacities for both runtimes on this host; equal-zero output is not backlog proof.

The first option check fails on an unused legacy wrapper; removing it restores the check. Retained later lifecycle probes still differ: new-server address(), missing listen generation, supplied-host pending state, positional callbacks and DNS-deferred conversion errors. A Symbol backlog probe prints accepted rows before Node throws asynchronously; synchronous try/catch assumptions are not valid. These remain separate from the passing syscall matrices and are not claimed fixed here.

A fresh unchanged all-nine one-second diagnostic keeps Express JSON invalid/unscored with 249 request timeouts; Hono/Fastify and Node/Bun targets are valid in that short run. Earlier f17 has Express 129 request timeouts and Hono 64 dial timeouts; snapshots include other shared source changes. No isolated attribution, production throughput/RSS improvement, authored optimization or win is claimed.

All 32 fixed-work captures against frozen 03ea582d2 are valid with identities unchanged; only two of eight 1% guards pass. Candidate build: iteration opt1, 256 codegen units, no LTO, incremental=false. Earlier snapshots used incremental compilation, so the frozen-executable comparison includes build-configuration differences and does not isolate source attribution.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}

Ordinary NUL string-key visibility, native-source/stack fidelity, complete listen lifecycle and the wider Node backlog remain open. This checkpoint does not establish total Node parity.
''')
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()for p in root.rglob('*')if p.is_file()and p.name!='sha256.json'};(root/'sha256.json').write_text(json.dumps(manifest,indent=2)+'\n');print(root)
