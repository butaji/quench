from pathlib import Path
import json,re,subprocess,shutil,hashlib
root=Path('evidence/web-operation-source-parity-2026-10-16');root.mkdir(exist_ok=True)
a=json.loads(Path('work/operation-source-qualification-after-api.json').read_text());assert a['api_exit_code']==0 and all(a['identity_checks'].values())
source=a['reconciled_candidate_source']
for name in ['operation-source-published-workspace','operation-source-published-runtime','operation-source-published-focused','operation-source-framework','operation-source-web','operation-source-api','operation-source-paired-callgrind']:assert Path('work/'+name+'.exit').read_text().strip()=='0',name
g=json.loads(Path('work/operation-source-paired-callgrind/report.json').read_text());assert g['all_valid'] and g['identities_unchanged'] and len(g['results'])==32
assert hashlib.sha256(Path(a['binaries']['cli']['path']).read_bytes()).hexdigest()==a['binaries']['cli']['sha256']
for pattern in ['operation-source-*.log','operation-source-*.exit','operation-source-qualification-*.json','operation-source-source.patch','operation-source-test-*.js','call-token-source-focused.log','call-token-source-focused.exit','call-token-source-repaired-focused.log','call-token-source-repaired-focused.exit','call-token-pc-debug.stdout','call-token-pc-debug.stderr','fused-field-position-probe.cjs','fused-field-position-*.stdout','fused-field-sink-probe.cjs','fused-field-sink-*.stdout']:
 for p in Path('work').glob(pattern):shutil.copy2(p,root/p.name)
for name in ['operation-source-web','operation-source-paired-callgrind']:shutil.copytree(Path('work')/name,root/name,dirs_exist_ok=True)
a.update(candidate_source=source,api_cases_passed=362,runtime_tests=653,framework_fixtures=43,web_correctness_targets=9,all_instrumented_executions_valid=True,instruction_limit_met=g['instruction_limit_met'],subsequent_fused_field_metadata_excluded=True)
(root/'qualification.json').write_text(json.dumps(a,indent=2)+'\n')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','e2e182a10',source,'--','crates','docs']))
rows='\n'.join(f"| {c['fixture']} | {c['delta_percent']:+.4f}% | {'pass' if c['within_limit'] else 'FAIL'} |"for c in g['comparisons'])
v=[c['candidate_startup']-c['base_startup']for c in g['comparisons']]
(root/'README.md').write_text(f'''# Executing operations and syntax-owned source locations

Frozen source {source} passes 362/362 serial API comparisons against official stable Node v26.12.0 (exit zero, 228.66 seconds), 653 Core tests, workspace/all-target checking, seven focused comparisons, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and API test hashes remain unchanged through the full run. The base e2e182a10, source patch and extra fixture hashes were reconciled before committing identical code. Subsequent fused-field/bytecode-format-91 changes are excluded.

Frames distinguish the executing guest-capable instruction from a saved continuation. Source capture projects the correct instruction without changing the GC cursor or lexical-binding owner. Call-state updates cover general and materialized lane calls; restored activations retain their continuation contract. A missing general-call update was caught by eval-root comparisons: file-entry calls matched, eval roots did not. The failure and temporary trace are retained; the common call path is fixed and temporary instrumentation removed before qualification.

OXC expression/comment spans identify real argument parentheses for computed, optional, private, parenthesized and function-valued calls. Comments containing parentheses are excluded by their parser-owned spans. Static member calls retain method-name positions. Reads and writes receive distinct positions for getters, setters, assignment, compound assignment and updates. Source lines recognize CR, LF, CRLF and Unicode separators; columns count UTF16 units. Oracle matrices cover these cases plus constructors, this(), returned functions and astral characters.

This candidate also includes shared native Buffer allocation/pooling. Both new Buffer oracle comparisons and their added runtime tests pass. Results are integrated measurements, not isolated attribution.

All 32 fresh strict matched Callgrind captures are valid against frozen 03ea582d2 with unchanged identities. These instruction checks are separate from production throughput/RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
{rows}

Startup increases by {min(v):,}–{max(v):,} instructions. No authored optimization, total Node parity or web benchmark win is claimed.

The retained fused-field probe shows this.a.b still attributes its first getter to b's source position. Source metadata for individual fused components is subsequent work. Full eval/sourceURL, source-map, Node host-stack, internal-key visibility and broader API conformance remain open. Missing frames and source locations are never fabricated.
''')
shutil.copy2('work/package-operation-source-evidence.py',root/'package-evidence.py')
