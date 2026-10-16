from pathlib import Path
import shutil,json,re,hashlib,subprocess
root=Path('evidence/web-callsite-parity-2026-10-16');root.mkdir(exist_ok=True)
api=Path('work/callsite-boundary-api.log').read_text();match=re.search(r'test result: ok\. (\d+) passed; 0 failed',api);assert match
assert Path('work/callsite-boundary-api.exit').read_text().strip()=='0'
assert Path('work/callsite-boundary-runtime.exit').read_text().strip()=='0'
j=json.loads(Path('work/callsite-boundary-qualification-before.json').read_text());after=json.loads(Path('work/callsite-boundary-qualification-after-api.json').read_text());assert all(after['identity_checks'].values())
g=json.loads(Path('work/callsite-boundary-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'];assert len(g['results'])==32
names=['callsite-probe-provenance.json','callsite-boundary-workspace.log','callsite-boundary-workspace.exit','callsite-boundary-runtime.log','callsite-boundary-runtime.exit','callsite-boundary-build.log','callsite-boundary-build.exit','callsite-boundary-api-build.log','callsite-boundary-api-build.exit','callsite-boundary-api.log','callsite-boundary-api.exit','callsite-boundary-framework.log','callsite-boundary-framework.exit','callsite-boundary-web.log','callsite-boundary-web.exit','callsite-boundary-qualification-before.json','callsite-boundary-qualification-after-api.json','callsite-boundary-paired-callgrind.log','callsite-boundary-paired-callgrind.exit','callsite-function-focused.log','callsite-function-focused.exit','node-tailframes-focused.log','node-tailframes-focused.exit','callsite-method-focused.log','callsite-method-focused.exit','callsite-prototype-focused.log','callsite-prototype-focused.exit','callsite-prototype-probe.cjs','callsite-function-probe.cjs','callsite-strict-probe.cjs','callsite-method-probe.cjs','function-factory-origin-probe.cjs','function-factory-origin-node.stdout','function-factory-origin-quench.stdout','util-callsites-probe.cjs','util-callsites-node.stdout','util-callsites-quench.stdout']
for name in names:shutil.copy2(Path('work')/name,root/name)
for p in Path('work').glob('callsite-boundary-focused-*'):shutil.copy2(p,root/p.name)
for p in Path('work').glob('callsite-boundary-native-*'):shutil.copy2(p,root/p.name)
for pattern in ['callsite-prototype-*.stdout','callsite-prototype-*.stderr','callsite-method-*.stdout','callsite-constructor-newtarget-*.stdout','callsite-constructor-newtarget-probe.cjs']:
 for p in Path('work').glob(pattern):shutil.copy2(p,root/p.name)
for name in ['callsite-boundary-web','callsite-boundary-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
j.update(api_cases_passed=int(match[1]),api_exit_code=0,runtime_tests=647,framework_fixtures=43,web_correctness_targets=9,all_instrumented_executions_valid=g['all_valid'],instruction_limit_met=g['instruction_limit_met'],later_factory_work_excluded=True)
assert hashlib.sha256(Path('work/quench-callsite-boundary-qualified').read_bytes()).hexdigest()==j['binaries']['cli']['sha256']
(root/'qualification.json').write_text(json.dumps(j,indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |" for c in g['comparisons'])
source=subprocess.check_output(['git','show',j['candidate_source']+':crates/quench-node-test/tests/child_entry.rs'],text=True);snippets=[]
for name in ['callsites_share_native_prototypes_and_supported_methods_preserve_descriptors_and_brands','callsite_function_identity_obeys_strict_frame_visibility_and_ignores_name_getters','callsite_method_names_follow_receiver_descriptors_without_getters_or_proxy_traps']:
 start=source.index('fn '+name);end=source.find('\n#[test]',start);snippets.append(source[start:end if end!=-1 else len(source)])
(root/'paired-contracts.rs.txt').write_text('\n'.join(snippets))
(root/'README.md').write_text(f'''# CallSite prototype, identity, receiver descriptors and strict frames

Frozen source {j['candidate_source']} passes {match[1]}/{match[1]} serial API comparisons against explicit stable Node v26.12.0 with exit zero, 647 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test executable hashes stayed fixed through the API run. The API run starts after compilation and Callgrind finish. Later factory/literal provenance work is excluded.

CallSites share a realm-owned native prototype. Their constructor throws Error("Not supported") on call/new without touching a Proxy newTarget's prototype. Native method descriptors and receiver errors match Node. The constructor's original identity survives public prototype.constructor mutation. Missing methods are not represented as completed APIs.

getFunction returns traced callable identity with strict-frame visibility shared by getThis. The first comparison failed: proper-tail-call elimination removed an observable strict Node caller. The Node host now explicitly retains those frames in both general and specialized dispatch; Core's default proper-tail-call contract and Wasm behavior remain intact. The failed comparison and subsequent passing control are both retained.

The initial 41e9b9f03 qualification failed: 340/343 API tests and three incomplete guard workloads hit a returning-call boundary panic. 53ba17cbd restores fallback when no specialized activation caller exists. All three affected API cases and Earley-Boyer/RayTrace/Richards native fixed work then pass, followed by this fresh complete qualification. The failed snapshot is preserved separately in web-callsite-initial-regression-2026-10-16; its incomplete counts are never used as instruction deltas.

getMethodName uses original creation names and safe receiver descriptor identity, strips accessor prefixes, searches inherited enumerable aliases and returns null for ambiguous distinct aliases. It observes current receiver properties after capture without invoking name accessors, descriptor getters or Proxy traps. Tests cover strict receivers, non-enumerable candidate matches, numeric keys and lone-surrogate names. Captured receivers/callees follow the existing shared stack root owner and its lifetime tests.

All 32 fresh strict matched Callgrind captures are valid with unchanged identities against frozen 03ea582d2. Instruction guards are separate from production throughput/RSS and do not establish a benchmark win.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

No total Node parity is claimed. Retained paired counterexamples show factory-created function constructor provenance and util.getCallSites placeholder names/positions remain incorrect. Complete stack/eval/source metadata and broad Node API conformance remain open. All original failed controls remain distinguished from repaired snapshot results.
''')
shutil.copy2('work/package-callsite-boundary-evidence.py',root/'package-evidence.py')
