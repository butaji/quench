from pathlib import Path
import shutil,json,hashlib
root=Path('evidence/web-icu-utf16-parity-2026-10-16');root.mkdir(exist_ok=True)
names=['icu78-build-provenance.json','icu78-build.log','icu78-build.exit','icu78-version-workspace.log','icu78-version-build.log','icu78-version-observed.log','icu78-final-api-build.log','icu78-final-api.log','icu78-final-api.exit','icu78-final-framework.log','icu78-final-web.log','icu78-paired-callgrind.log','icu78-qualification-before.json','icu78-qualification-after-api.json','icu78-portable-check.log','icu78-portable-check.exit','icu78-decoder-focused.log','icu78-decoder-focused.exit','icu78-decoder-node26-focused.log','icu78-decoder-node26-focused.exit','decoder-oracle-correction.txt','utf16-encoding-runtime.log','utf16-encoding-runtime.exit','icu78-qualified-push.log','utf16-error-stack-probe.cjs','utf16-error-stack-expanded.cjs']
for name in names:shutil.copy2(Path('work')/name,root/name)
for name in ['icu78-final-web','icu78-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
for p in Path('work').glob('utf16-error-stack-*.stdout'):shutil.copy2(p,root/p.name)
for p in Path('work').glob('utf16-error-stack-*.stderr'):shutil.copy2(p,root/p.name)
j=json.loads(Path('work/icu78-qualification-before.json').read_text());g=json.loads(Path('work/icu78-paired-callgrind/report.json').read_text());j.update(api_cases_passed=334,api_exit_code=0,runtime_tests=644,framework_fixtures=43,web_correctness_targets=9,all_instrumented_executions_valid=g['all_valid'],instruction_limit_met=g['instruction_limit_met'],publication='atomic push rejected; neither remote ref updated; later integration excluded')
assert hashlib.sha256(Path('work/quench-icu78-qualified').read_bytes()).hexdigest()==j['binaries']['cli']['sha256']
(root/'qualification.json').write_text(json.dumps(j,indent=2)+'\n')
(root/'README.md').write_text('''# Pinned ICU, canonical encoding and UTF16 error behavior

Frozen source d40beb63f passes 334/334 serial API comparisons against explicit stable Node v26.12.0 with exit zero, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. The 644-test Core result is from 820549d7a; Core source is unchanged through d40beb63f. CLI, runner and test executable hashes stayed fixed through the API run. Later shared CallSite prototypes and provider/generator integration are excluded.

The unchanged configured internal/encoding.js uses the incoming native ICU bridge. The pinned Node source builds ICU 78.3; its data SHA256 matches the official stable Node archive. The retained build provenance identifies source, compiler, platform, library and data hashes. process.versions.icu now reports the compiled version instead of the old fixed 75.1. Portable no-ICU compilation passes and omits the false ICU version.

The first focused ICU decoder comparison accidentally used an older default Node, producing a failed encoder-symbol descriptor comparison. That failed run is retained and explicitly superseded by the successful four-test run with the Node v26.12.0 binary on PATH. No production workaround was added for the oracle mismatch.

Error messages, Error.prototype.toString, default stack headers and CallSite.toString preserve UTF16 code units. Capture targets observe the original name/message getters and caching, including throwing getters. Both initial counterexamples and expanded probe input are included. The expanded input also completes the earlier native-inspection evidence bundle's probe context; results are not rewritten.

All 32 fresh matched Callgrind captures are valid with stable binary identities. Six instruction guards pass; Earley-Boyer +2.3496% and Splay +2.3675% fail the 1% guard. These are integrated findings without isolated attribution or production throughput/RSS claims. They remain open in TOD-135. This evidence does not establish total Node parity or a web benchmark win.

The atomic push of this qualified ancestor was rejected when v2-merged advanced. Neither remote ref changed; the retained push log is evidence of non-publication, not success. Subsequent integration must be qualified separately.
''')
shutil.copy2('work/package-icu78-evidence.py',root/'package-evidence.py')
