from pathlib import Path
import json,re,shutil,subprocess,hashlib
root=Path('evidence/web-real-callsite-parity-2026-10-16');root.mkdir(exist_ok=True)
a=json.loads(Path('work/detailed-stack-qualification-after-api.json').read_text());assert a['api_exit_code']==0 and all(a['identity_checks'].values())
source=a['reconciled_candidate_source'];assert Path('work/detailed-stack-published-runtime.exit').read_text().strip()=='0'
for phase in ['framework','web','api','paired-callgrind']:assert Path('work/detailed-stack-'+phase+'.exit').read_text().strip()=='0',phase
assert Path('work/detailed-stack-generator-cleanup-subset.exit').read_text().strip()=='0'
g=json.loads(Path('work/detailed-stack-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
for pattern in ['detailed-stack-*.log','detailed-stack-*.exit','detailed-stack-qualification-*.json','detailed-stack-source.patch','detailed-stack-test-*.js','detailed-stack-oracle-probe.cjs','detailed-stack-oracle-probe.stdout','detailed-stack-oracle-probe.stderr','call-column-probe.cjs','call-column-node.stdout','call-column-quench.stdout','optional-private-call-columns-probe.cjs','optional-private-columns-*.stdout','call-column-trivia-probe.cjs','call-column-trivia-*.stdout','complex-call-column-probe.cjs','complex-call-column-*.stdout','special-call-column-probe.cjs','special-call-column-*.stdout','getter-call-column-probe.cjs','getter-call-column-*.stdout','property-access-column-probe.cjs','property-access-column-*.stdout']:
 for p in Path('work').glob(pattern):shutil.copy2(p,root/p.name)
for name in ['detailed-stack-web','detailed-stack-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=356,runtime_tests=651,framework_fixtures=43,web_correctness_targets=9,generator_cleanup_regression_passed=True,all_instrumented_executions_valid=True,instruction_limit_met=g['instruction_limit_met'],later_operation_pc_and_token_location_work_excluded=True)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n')
for name in ['test-callsite-call-columns.js','test-util-real-call-sites.js']:(root/name).write_bytes(subprocess.check_output(['git','show',source+':crates/quench-node-test/node-tests/'+name]))
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','dbac9a481',source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons'])
(root/'README.md').write_text(f'''# Actual detailed source frames and call positions

Frozen source {source} passes 356/356 serial API comparisons against official stable Node v26.12.0 (exit zero, 223.73 seconds), 651 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. The shared generator-pattern iterator-cleanup regression also passes as an isolated subset test. CLI, runner and test hashes remain unchanged through the full API run. The recorded base dbac9a481, source patch and fixture hashes were reconciled before committing identical source. Later operation-PC/token-position changes are excluded.

The public util.getCallSites function comes from unmodified configured Node util.js. Its native operation projects actual JavaScript activations independently of Error hooks/limits, skips the utility wrapper, omits native helper frames and returns null-prototype records with copied UTF16 names, source identities, filenames, positions and the column alias. Source identities derive from actual compiled units and are not fabricated from argv. Error CallSites and detailed capture share source projection; host filename mapping remains an explicit mutable boundary.

The oracle comparison covers mutable/accessor-fallback/UTF16 names, positive source identities and equality within a source, exact first-frame positions, descriptors/key order, Error-hook independence, validation and the original function's reflection/source. The implemented call-column comparison covers six identifier/member/computed/parenthesized/constructor forms. These selected passes do not establish complete stack conformance.

Initial failures are retained: a read-only helper could not call the mutable host filename mapper; the first public comparison failed because the configured-library binding bridge did not expose the new native operation. Explicit host-effect handling and the missing delegate repair both issues. After the shared lazy-library/transition-index merge, workspace checks pass, but the first harness build exhausts disk while LLVM writes bitcode. Its diagnostics are build/environment failures, not test results. Inactive previous-graph caches were removed; the retry and both focused comparisons pass before full qualification.

All 32 fresh strict matched Callgrind captures are valid against frozen 03ea582d2, with unchanged identities. Seven guards pass; Earley-Boyer remains above 1%. Integrated results include shared lazy library loading and transition-index changes and do not isolate attribution.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by 17,782,525–230,597,788 instructions. No production throughput/RSS, authored optimization, total Node parity or web benchmark win is claimed.

Retained new negative controls show optional/private/complex calls and comments before call parentheses still have incorrect positions. Getter/setter probes also reveal that source capture subtracts one from a PC which sometimes denotes the executing operation rather than a saved call continuation. Those repairs are subsequent work. Complete source-map/eval/sourceURL/host-stack support, internal-key visibility, allocation provenance and broader Node APIs remain open. Missing Node host frames are not fabricated.
''')
shutil.copy2('work/package-detailed-stack-evidence.py',root/'package-evidence.py')
