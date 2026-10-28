from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-native-net-http-lifecycle-parity-2026-10-16')
before=json.loads(Path('work/net-server-http-handle-qualification-before.json').read_text());after=json.loads(Path('work/net-server-http-handle-qualification-after-api.json').read_text());assert before==after and not before['status'];source=before['candidate_source']
for p,h in before['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
for k in ['cli','node']:
 b=before['binaries'][k];assert hashlib.sha256(Path(b['path']).read_bytes()).hexdigest()==b['sha256'],k
for name in ['api','workspace','framework','web','paired-callgrind']:assert Path('work/net-server-http-handle-'+name+'.exit').read_text().strip()=='0',name
assert Path('work/net-server-lifecycle-captured-runtime.exit').read_text().strip()=='0'
assert not subprocess.check_output(['git','diff','409e83ee9',source,'--','crates/quench-runtime','Cargo.toml','Cargo.lock'])
r=json.loads(Path('work/net-server-http-handle-paired-callgrind/report.json').read_text());assert r['all_valid']and r['identities_unchanged']and len(r['results'])==32
root.mkdir(exist_ok=True)
for pattern in ['net-server-http-handle-*','net-server-lifecycle-*','net-server-dns-iterator-errors-*','net-server-close-twice-*','net-server-listening-getter-shape-*','net-server-method-inspection-*','net-server-function-shapes-*','process-capture-clear-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
for name in ['web','paired-callgrind']:shutil.copytree(Path('work/net-server-http-handle-'+name),root/name,dirs_exist_ok=True)
for name in ['net-listen-lifecycle','net-dns-iterator-errors','process-capture-callback-reset','http-listening-handle']:
 shutil.copy2('crates/quench-node-test/node-tests/test-'+name+'.js',root/('oracle-'+name+'.js'))
shutil.copy2(__file__,root/'package-evidence.py');(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','88a5c9d57',source,'--','crates','docs']))
summary=before|{'api_cases_passed':387,'runtime_tests':668,'runtime_tree_verified_from':'409e83ee9','runtime_seconds':64.08,'framework_fixtures':43,'web_correctness_targets':9,'passed_instruction_guards':sum(c['within_limit']for c in r['comparisons']),'total_node_parity':False,'throughput_win':False,'candidate_build':{'profile':'iteration','opt_level':1,'codegen_units':256,'lto':False,'incremental':False},'source_only_build_matched_comparison':False}
(root/'qualification.json').write_text(json.dumps(summary,indent=2)+'\n');rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass'if c['within_limit']else'FAIL'} |"for c in r['comparisons'])
(root/'README.md').write_text(f'''# Native net lifecycle and one authoritative HTTP listening handle

Frozen source {source} passes 387 serial Node v26.12.0 API comparisons, workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes remain unchanged across APIs. Runtime source matches 409e83ee9, whose 668 tests pass (64.08s), including expanded ordinary native-constructor metadata with traced data. Later changes are excluded.

Rust receives the complete net.Server.listen argument list. Public method/getter/callback constructor shapes match the oracle; creation names are distinct from private dispatch names. Supplied hosts use the existing public DNS module, so overrides and next-tick timing remain observable. Callbacks retain traced data and compare the actual public listen generation. Tests cover wildcard/host pending state, positional callback/backlog, null new-server address, cancellation, repeated close, DNS iteration and early-return cleanup, callback shape/return value, and GC before callback delivery.

Getter failure closes the DNS iterator while preserving its original RangeError and symbol through GC, even when return also throws. Normal iterator-return failure remains observable. The probe exposed a separate occupied exception-capture slot bug: null reset now precedes the occupied check, invalid types precede duplicate-state errors, and shared coded/type errors preserve original scalar descriptions and messages.

The first full 409e run fails six of 386 APIs (380 pass): four HTTP cases and both HTTP-dependent syscall matrices. The new inherited listening getter rejects legacy HTTP writes to a duplicate boolean. That failed qualification is retained separately in web-net-lifecycle-http-initial-regression-2026-10-16. HTTP now derives listening from its actual handle, clears that handle on close and cancels a pending listening notification. An intermediate binary still emits listening after early close; the explicit Node regression catches it and the corrected full run passes. Raw early callback-name, repeated-close, capture-reset and HTTP failures remain separate from successful results.

All 32 matched fixed-work captures are valid with unchanged identities against frozen 03ea582d2. Candidate uses iteration opt1/256 CGUs/no LTO/incremental=false; earlier snapshots used incremental compilation. Integrated frozen-executable deltas include build-configuration differences and do not isolate source attribution or production throughput/RSS.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
{rows}

Bound-handle error/reentry details, original JavaScript function-source/native-host-frame fidelity, diagnostic report flags, ordinary NUL/internal-property namespaces and wider Node APIs remain open. No total parity, authored optimization or benchmark win is claimed.
''')
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()for p in root.rglob('*')if p.is_file()and p.name!='sha256.json'};(root/'sha256.json').write_text(json.dumps(manifest,indent=2)+'\n');print(root)
