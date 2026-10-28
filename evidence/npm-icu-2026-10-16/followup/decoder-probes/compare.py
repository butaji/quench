import json,subprocess
from pathlib import Path
p=Path(__file__).parent
allrows=[json.loads(s) for s in (p/'transcode-input.jsonl').read_text().splitlines()]
# The public Buffer policy returns before native transcode for empty input.
# Direct native empty input hits a Node assertion; preserve that failed probe.
rows=[r for r in allrows if r['bytes']]
width=[dict(op='width',units=[cp],ambiguous=a,expand=e) for cp in range(65536) for a,e in [(False,True),(True,False)]]
for seq in [[0xd83d,0xdc68,0x200d,0xd83d,0xdc69,0x200d,0xd83d,0xdc67],[0xd83c,0xdffb],[0xad],[0xd800,0x200d,0xdc00]]:
 for a in [False,True]:
  for e in [False,True]: width.append(dict(op='width',units=seq,ambiguous=a,expand=e))
for label,rows in [('transcode-native-nonempty',rows),('width',width)]:
 data=''.join(json.dumps(r)+'\n' for r in rows);(p/f'{label}-input.jsonl').write_text(data)
 for side,args in [('node',['/workspace/shared/node-v26.12.0-linux-x64/bin/node','--expose-internals',str(p/'native-oracle.cjs')]),('draft',[str(p/'probe')])]:
  r=subprocess.run(args,input=data,text=True,capture_output=True);(p/f'{label}-{side}.jsonl').write_text(r.stdout);(p/f'{label}-{side}.stderr').write_text(r.stderr);assert r.returncode==0,(label,side,r.returncode)
 left=[json.loads(s) for s in (p/f'{label}-node.jsonl').read_text().splitlines()];right=[json.loads(s) for s in (p/f'{label}-draft.jsonl').read_text().splitlines()];assert len(left)==len(right)==len(rows)
 diffs=[dict(input=row,node=a,draft=b) for row,a,b in zip(rows,left,right) if a!=b];(p/f'{label}-finding.json').write_text(json.dumps({'cases':len(rows),'differences':len(diffs),'rows':diffs},indent=2)+'\n');print(label,len(rows),len(diffs),diffs[:2])
