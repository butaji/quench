"""Resume a checkpoint only when engine, source, inventory and fixture identities agree.
Usage: python3 resume-packages.py REPORT_JSON
"""
import json
from pathlib import Path
import subprocess
import sys

repo = Path(__file__).resolve().parents[2]
path = Path(sys.argv[1]).resolve()
report = json.loads(path.read_text())
parts = path.with_suffix('.parts')
parts.mkdir(exist_ok=True)
inventory = json.loads((repo / 'tests/npm-top300/inventory.json').read_text())['packages']
seen = {row['package']['name'] for row in report['records']}
assert len(seen) == len(report['records'])
identity = ['mode','source_commit','source_dirty','node_sha256','quench_sha256','inventory_sha256','lock_sha256','scenario_sha256','timeout_seconds']
for package in inventory:
    name = package['name']
    if name in seen:
        continue
    one = parts / f'{len(seen):03d}.json'
    args = [str(repo/'target/iteration/run-npm'),report['node_executable'],report['quench_executable'],str(one),str(report['timeout_seconds'])]
    if report['mode'] == 'surface': args.append('--surface')
    args.append(name)
    run = subprocess.run(args,cwd=repo,capture_output=True,timeout=2*report['timeout_seconds']+30)
    data = json.loads(one.read_text())
    for key in identity: assert data[key] == report[key], (name,key)
    assert len(data['records']) == 1 and data['records'][0]['package']['name'] == name
    report['records'].extend(data['records'])
    seen.add(name)
    order = {p['name']:i for i,p in enumerate(inventory)}
    report['records'].sort(key=lambda row:order[row['package']['name']])
    path.write_text(json.dumps(report,indent=2)+'\n')
    print(name,data['records'][0]['verdict'],flush=True)
assert len(report['records']) == 300
