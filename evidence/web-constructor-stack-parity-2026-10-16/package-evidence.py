from pathlib import Path
import hashlib,json,re,shutil,subprocess
root=Path('evidence/web-constructor-stack-parity-2026-10-16')
manifest=json.loads(Path('work/array-class-repair-qualification-before.json').read_text())
after=json.loads(Path('work/array-class-repair-qualification-after-api.json').read_text())
assert all(after['identity_checks'].values())
api=Path('work/array-class-repair-api.log').read_text();m=re.search(r'test result: ok\. (\d+) passed; 0 failed',api);assert m
assert Path('work/array-class-repair-api.exit').read_text().strip()=='0'
for phase in ['workspace','runtime','focused','framework','web']:
 assert Path('work/array-class-repair-'+phase+'.exit').read_text().strip()=='0',phase
assert hashlib.sha256(Path(manifest['binaries']['cli']['path']).read_bytes()).hexdigest()==manifest['binaries']['cli']['sha256']
guard=json.loads(Path('work/array-class-repair-paired-callgrind/report.json').read_text())
assert guard['all_valid'] and guard['identities_unchanged'] and len(guard['results'])==32
root.mkdir(exist_ok=True)
for pattern in ['array-class-repair-*.log','array-class-repair-*.exit','array-class-repair-qualification-*.json','node-inferred-name-state.cjs','node-inferred-name-state.stdout','node-inferred-name-prototype.cjs','node-inferred-name-prototype.stdout','constructor-class-probe.cjs','function-factory-node-expanded-focused.log','function-factory-node-expanded-focused.exit','inferred-name-gc-focused.log','inferred-name-gc-focused.exit']:
 for p in Path('work').glob(pattern):shutil.copy2(p,root/p.name)
for name in ['array-class-repair-web-2','array-class-repair-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
source=manifest['candidate_source']
for name in ['test-callsite-type-construction.js','test-inferred-function-debug-names.js','test-constructor-class-data.js']:
 data=subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/'+name]);(root/name).write_bytes(data)
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','e883a6722',source,'--','crates','docs']))
manifest.update(api_cases_passed=int(m[1]),api_exit_code=0,runtime_tests=649,framework_fixtures=43,web_correctness_targets=9,all_instrumented_executions_valid=True,instruction_limit_met=guard['instruction_limit_met'],boxed_primitive_work_excluded=True)
(root/'qualification.json').write_text(json.dumps(manifest,indent=2)+'\n')
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |" for c in guard['comparisons'])
(root/'README.md').write_text(f'''# Function origins and constructor stack metadata

Frozen source {source} passes {m[1]}/{m[1]} serial API comparisons against official stable Node v26.12.0, 649 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and API test hashes remain unchanged through the full API run. Separate boxed-primitive follow-up work is excluded.

Function factory constructors derive from immutable original realm intrinsics, function kind and creation realm. Tests cover main/foreign ordinary, arrow, async and generator functions, native and bound functions, mutated public links and safe debug names. Iterator constructor display uses actual hidden origins and physical prototype roles. Completed generator instances retain their creating function until the instance dies; collection tests cover both execution modes.

CallSite constructor flags derive from actual activation entry in general and materialized specialized frames. Pending construction targets belong to the actual callee. Receiver type names share safe Core constructor display and preserve constructor/global null results, primitive receivers and Proxy trap avoidance. Constructor rendering retains the new prefix.

Variable-initializer debug-name inference flows through result-producing sequences, logical/coalescing expressions and conditionals. It does not change the public name. Slotless shape transitions track native name-property reconfiguration and survive compaction. Reading a name or descriptor preserves that state; replacing its value/attributes materializes the public name. Inference outside those contexts remains open. Bytecode format 90 supports the new metadata opcode; checked dense dispatch falls back for later opcodes.

The initial 992e960e2 snapshot failed 1/348 API comparisons: detached arrays in the direct-record arena reported Object. Its full failed run and fixed hash checks are retained in ../web-constructor-stack-initial-regression-2026-10-16. df33c1767 uses the authoritative array projection and passes the affected case before this fresh complete qualification. Earlier focused constructor comparison also caught missing inferred names; the repaired fixture retains that case.

The first framework invocation incorrectly inherited the child-runner environment, and the first web invocation lacked the compiler PATH. Their diagnostics are retained as invocation errors, not runtime regressions. Corrected framework invocation unsets QUENCH_CHILD_RUNNER; web verification uses the build environment and a fresh output directory.

All 32 fresh strict matched Callgrind captures are valid against frozen 03ea582d2, with unchanged candidate/baseline identities. Startup-subtracted instruction work is separate from throughput, RSS and benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

No total Node parity or web benchmark win is claimed. Native factory/boxing provenance, broader inferred names, complete stack/eval metadata, util.getCallSites, boxed primitive forgery, internal-key visibility and broad Node APIs remain open at this frozen source. The confirmed boxed-primitive repair is subsequent work.
''')
shutil.copy2('work/package-constructor-stack-evidence.py',root/'package-evidence.py')
