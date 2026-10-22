from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('evidence/web-net-lifecycle-http-initial-regression-2026-10-16');before=json.loads(Path('work/net-server-lifecycle-qualification-before.json').read_text());after=json.loads(Path('work/net-server-lifecycle-qualification-after-api.json').read_text());assert before==after and not before['status'];source=before['candidate_source'];assert Path('work/net-server-lifecycle-api.exit').read_text().strip()=='101'
for p,h in before['source_files'].items():assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+p])).hexdigest()==h,p
root.mkdir(exist_ok=True)
for pattern in ['net-server-lifecycle-*','net-server-dns-iterator-errors-*','net-server-close-twice-*','net-server-listening-getter-shape-*','net-server-method-inspection-*','net-server-function-shapes-*','process-capture-clear-*']:
 for p in Path('work').glob(pattern):
  if p.is_file():shutil.copy2(p,root/p.name)
shutil.copy2(__file__,root/'package-evidence.py')
(root/'qualification.json').write_text(json.dumps(before|{'api_cases_passed':380,'api_cases_failed':6,'api_exit':101,'api_seconds':260.45,'candidate_qualified':False,'later_http_handle_correction_excluded':True},indent=2)+'\n')
(root/'source-change.patch').write_bytes(subprocess.check_output(['git','diff','88a5c9d57',source,'--','crates','docs']))
(root/'README.md').write_text(f'''# Initial native net lifecycle exposes duplicate HTTP listening state

Frozen source {source} fails six of 386 serial Node v26.12.0 API comparisons: 380 pass, six fail, exit 101 in 260.45 seconds. Source/oracle/CLI/runner/test identities remain unchanged across the run. This is a failed qualification, not parity or performance evidence.

Four HTTP response/wire tests fail while constructing a server; both TCP syscall tests also fail in their HTTP cases. The new Node-compatible inherited listening getter derives from _handle. Legacy HTTP code still writes a separate listening boolean, producing "cannot install shared http property listening". The subsequent correction removes those duplicate writes and clears the actual handle on close; that later source and its results are excluded here.

Focused native lifecycle, DNS iterator/GC and capture-reset cases pass before this full failure. Earlier first-binary callback-name mismatch, missing repeated-close callback, and occupied capture-slot reset failure remain diagnostic records. Native function/source/host-frame fidelity and broader Node behavior remain open. No total parity, optimization or web win is claimed.
''')
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()for p in root.rglob('*')if p.is_file()and p.name!='sha256.json'};(root/'sha256.json').write_text(json.dumps(manifest,indent=2)+'\n');print(root)
