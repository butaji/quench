import json, subprocess
from pathlib import Path
repo=Path('/workspace/quench'); scratch=Path('/workspace/scratch')
for mode in ['scenarios','surface']:
    path=scratch/f'npm-capabilities-published-{mode}.json'; report=json.loads(path.read_text())
    remaining=json.loads((scratch/f'npm-capabilities-remaining-{mode}.json').read_text())
    for index,name in enumerate(remaining):
        one=scratch/f'npm-capabilities-resumed-{mode}-{index}.json'
        command=[str(repo/'target/iteration/run-npm'),'/workspace/shared/node-v26.11.1-linux-x64/bin/node','/workspace/scratch/npm-capabilities-final-candidate',str(one),'120']
        if mode=='surface':command.append('--surface')
        command.append(name)
        result=subprocess.run(command,cwd=repo,capture_output=True,text=True,timeout=270)
        data=json.loads(one.read_text())
        for key in ['mode','inventory_sha256','lock_sha256','node_sha256','quench_sha256','source_commit','source_dirty']:
            assert data[key]==report[key],(name,key)
        assert len(data['records'])==1 and data['records'][0]['package']['name']==name
        report['records'].extend(data['records']);path.write_text(json.dumps(report,indent=2)+'\n')
        print(mode,name,data['records'][0]['verdict'],flush=True)
    order={r['name']:i for i,r in enumerate(json.loads((repo/'tests/npm-top300/inventory.json').read_text())['packages'])}
    report['records'].sort(key=lambda r:order[r['package']['name']]);path.write_text(json.dumps(report,indent=2)+'\n')
    assert len(report['records'])==300
