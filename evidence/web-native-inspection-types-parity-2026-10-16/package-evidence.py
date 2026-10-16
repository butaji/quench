from pathlib import Path
import shutil,json,hashlib
root=Path('evidence/web-native-inspection-types-parity-2026-10-16');root.mkdir(exist_ok=True)
for name in ['native-types-final-workspace.log','native-types-final-runtime.log','native-types-final-build.log','native-types-final-api-build.log','native-types-final-api.log','native-types-final-framework.log','native-types-final-web.log','native-types-paired-callgrind.log','native-types-qualification-before.json','native-types-qualification-after-api.json','native-types-api-session-transport.txt','lazy-stack-workspace.log','lazy-stack-workspace-2.log','lazy-stack-gc-focused.log','lazy-stack-name-focused.log','native-constructor-focused.log','native-constructor-focused-2.log','canonical-util-types-focused.log','canonical-util-types-focused-2.log']:
 shutil.copy2(Path('work')/name,root/name)
for name in ['native-types-final-web','native-types-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
j=json.loads(Path('work/native-types-qualification-before.json').read_text());g=json.loads(Path('work/native-types-paired-callgrind/report.json').read_text());j.update(api_cases_passed=330,api_terminal_exit_packet='unavailable_after_transport_disconnect',runtime_tests=643,framework_fixtures=43,web_correctness_targets=9,all_instrumented_executions_valid=g['all_valid'],instruction_limit_met=g['instruction_limit_met'],later_utf16_error_work_excluded=True)
j['candidate_sha256_after']=hashlib.sha256(Path('work/quench-native-types-qualified').read_bytes()).hexdigest();assert j['candidate_sha256_after']==j['binaries']['cli']['sha256'];j['canonical_types_source']={}
for name,p in [('configured','tests/node/lib/internal/util/types.js'),('stable','work/node_modules/node-stable-conformance/lib/internal/util/types.js')]:j['canonical_types_source'][name]=hashlib.sha256(Path(p).read_bytes()).hexdigest()
assert len(set(j['canonical_types_source'].values()))==1
(root/'qualification.json').write_text(json.dumps(j,indent=2)+'\n')
probes=root/'counterexamples-on-frozen-source';probes.mkdir(exist_ok=True)
for pattern in ['utf16-error-stack-probe.cjs','utf16-error-stack-*.stdout','utf16-error-stack-*.stderr','callsite-prototype-probe.cjs','callsite-prototype-*.stdout','callsite-prototype-*.stderr']:
 for path in Path('work').glob(pattern):shutil.copy2(path,probes/path.name)
(root/'README.md').write_text('''# Native constructor provenance, lazy stack names and canonical type predicates

Frozen source 6a8ef406b passes 643 runtime tests, workspace/all-target checking, 43 framework fixtures and all nine no-JIT web correctness targets. The complete serial API log reports all 330 cases passed. CLI, runner and test executable hashes are unchanged before/after that run. The execution transport disconnected after the complete log was written, so the final session exit packet could not be recovered; this is explicitly retained rather than inventing an exit status. Later UTF16/error-header work and shared encoding integration are excluded.

Captured errors and exposed CallSite objects now trace callable identity and derive safe function debug names at read time. Paired controls cover mutation both before and after stack materialization, empty/NUL/lone-surrogate names and throwing name getters. Core tests in both execution modes verify retention through captured errors and exposed callsites, then release when the last stack owner disappears.

Native new allocations preserve actual base constructor identity while Object(existing) retains its result origin. Native subclass display follows prototype/tag lookup before intrinsic class fallback; foreign-realm native names survive prototype detachment. The first native matrix matched all but Error: its bootstrap had assigned a name outside the shared creation-name operation. Error classes now use that operation, and the expanded standard-error matrix passes.

Public util.types uses unchanged configured internal/util/types.js, byte-identical to stable Node v26.12.0. It owns JavaScript predicate source, aliases, constructibility, export order and crypto wrappers. Raw native predicates use the exact 26-entry node_types.cc subset/order and anonymous callbacks with private brand data. The first complete reflection check exposed arity 1 instead of 0; the declaration is repaired. Complete public/private source, identity, arity, descriptors, prototype presence and key ordering now match the oracle. Duplicate Rust public crypto wrapper construction is removed.

All 32 fresh strict matched Callgrind captures are valid with stable identities against frozen 03ea582d2. Earley-Boyer +2.2986% and Splay +2.3037% exceed the 1% limit. Six pass: Crypto +0.1146%, DeltaBlue -5.2771%, Navier-Stokes -1.0464%, RayTrace -2.0591%, RegExp -7.2284%, Richards -6.6504%. This is integrated functional evidence, not isolated attribution, production throughput/RSS evidence or a web win. TOD-135 remains open.

Total Node parity is not established. Retained frozen counterexamples include lone-surrogate error messages/rendering and CallSite prototype/API differences. Later error-header/UTF16 repair is excluded. Native factory/literal provenance, complete eval wrapper frames and the broad stable diagnostic audit remain open; selected passing tests do not replace full conformance.
''')
shutil.copy2('work/package-native-types-evidence.py',root/'package-evidence.py')
