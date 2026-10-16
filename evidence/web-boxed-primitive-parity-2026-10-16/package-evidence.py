from pathlib import Path
import json,re,subprocess,shutil,hashlib
root=Path('evidence/web-boxed-primitive-parity-2026-10-16');root.mkdir(exist_ok=True)
a=json.loads(Path('work/boxed-slots-published-qualification-after-api.json').read_text());assert all(a['identity_checks'].values()) and a['api_exit_code']==0
source=a['reconciled_candidate_source'];api=Path('work/boxed-slots-published-api.log').read_text();m=re.search(r'test result: ok\. (\d+) passed; 0 failed',api);assert m
for phase in ['workspace','runtime','focused','framework','web','paired-callgrind']:assert Path('work/boxed-slots-published-'+phase+'.exit').read_text().strip()=='0',phase
g=json.loads(Path('work/boxed-slots-published-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
for pattern in ['boxed-slots-published-*.log','boxed-slots-published-*.exit','boxed-slots-published-*.json','boxed-slots-published-source.patch','boxed-slots-published-fixture.js','boxed-slots-runtime.log','boxed-slots-runtime.exit','boxed-slots-focused.log','boxed-slots-focused.exit','boxed-slots-focused-provenance.json','boxed-slots-build.patch','boxed-slots-fixture.js','boxed-slots-integrated-runtime.log','boxed-slots-integrated-runtime.exit','boxed-slots-integrated-source.json','boxed-slots-integrated-source.patch','boxed-slots-integrated-fixture.js','boxed-brand-forgery-probe.cjs','boxed-brand-forgery-*.stdout','boxed-brand-forgery-*.stderr','boxed-brand-forgery-*.exit','internal-string-key-visibility-probe.cjs','internal-string-key-*.stdout','internal-string-key-*.stderr','internal-string-key-*.exit','boxed-and-internal-key-probe-provenance.json']:
 for p in Path('work').glob(pattern):shutil.copy2(p,root/p.name)
for name in ['boxed-slots-published-web','boxed-slots-published-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
for codec in ['brotli','zlib']:shutil.copy2(Path('work')/(codec+'-node')/'provenance.json',root/(codec+'-sdk-provenance.json'))
shutil.copy2('work/integrated-compression-stable-source-check.json',root/'compression-source-check.json')
a.update(candidate_source=source,api_cases_passed=int(m[1]),runtime_tests=651,framework_fixtures=43,web_correctness_targets=9,all_instrumented_executions_valid=True,instruction_limit_met=g['instruction_limit_met'],subsequent_detailed_stack_work_excluded=True)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n')
(root/'test-boxed-primitive-slots.js').write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/test-boxed-primitive-slots.js']))
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','73bd7e869',source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons'])
(root/'README.md').write_text(f'''# Genuine boxed primitive data

Frozen source {source} passes {m[1]}/{m[1]} serial API comparisons against official stable Node v26.12.0 (exit zero, 227.10 seconds), 651 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes remain unchanged through the API run. Source was built from 73bd7e869 plus the recorded patch and extra fixture; their hashes were reconciled before committing the identical source. Subsequent detailed-stack/call-column work is excluded.

The retained initial probe proves that ordinary objects can forge all five wrapper brands by setting internal-looking public string properties. Node rejects them; the old Quench snapshot accepts them and lets primitive methods read forged values. The repair stores one genuine primitive Value in object extras, then derives brands, primitive methods, constructor display, structured cloning and JSON unboxing from it. Public properties cannot supply or replace that value; Proxy targets do not donate their brand. Redundant marker atoms and JSON wrapper-key caches are removed. BigInt valueOf shares one implementation.

The expanded oracle comparison covers forged objects, all wrong wrapper kinds, detached real wrappers, public-key replacement, Proxy trap avoidance, cloning, intrinsic prototype brands and JSON. A weak-reference lifetime test in both execution modes keeps a detached wrapper's symbol alive through collection, then proves release after dropping the wrapper. The common extras visitor owns the payload root.

Before shared integration, 650 Core tests and the focused Node comparison pass. The first integrated runtime build exits 101 before running tests because the 32 GiB workspace runs out of space. Its compiler diagnostics and source patch are retained. Disposable compiler caches were removed; the published-source retry then passes all 651 tests. This environment/build failure is not represented as a runtime-test regression.

This candidate also includes shared native compression, Node stack/error policy and generator-pattern cleanup already merged through 73bd7e869. Matching codec SDK/compiler/archive provenance and stable-source comparisons are retained. Startup and instruction changes are integrated results, not isolated attribution to boxing.

All 32 fresh strict matched Callgrind captures are valid with unchanged identities against frozen 03ea582d2. Six instruction guards pass; Earley-Boyer and Splay remain above the 1% limit.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by 326,284,104–683,528,137 instructions. This is not production throughput/RSS evidence or a benchmark win. No authored optimization or total Node parity is claimed.

An independent retained probe confirms that ordinary user keys beginning with the internal prefix are hidden by reflection and JSON. That separate namespace problem remains open. Native factory/boxing constructor provenance, complete stack/eval/source metadata, broader inference, HTTP lifecycle/HTTP2, workers, shared memory and other Node APIs also remain open. util.getCallSites still has placeholder behavior in this frozen candidate; its later repair is excluded here.
''')
shutil.copy2('work/package-boxed-slots-evidence.py',root/'package-evidence.py')
