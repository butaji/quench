from pathlib import Path
import hashlib,json,subprocess,tempfile,time
s=Path('/workspace/scratch');prefix=s/'npm-functions'
try:
    while not Path(f'{prefix}-node-lib.exit').exists():
        failure=Path(f'{prefix}-build-checks.exit')
        if failure.exists()and failure.read_text().strip()!='0':raise RuntimeError('build prerequisites failed')
        time.sleep(2)
    assert Path(f'{prefix}-node-lib.exit').read_text().strip()=='0'
    engines=json.loads(Path(f'{prefix}-engines.json').read_text())
    cli=s/'npm-functions-candidate';node=Path(engines['node']['path'])
    assert hashlib.sha256(cli.read_bytes()).hexdigest()==engines['candidate']['sha256']
    rows=[]
    for label,script,count in [('math-bit-dense',s/'npm-node-math-bit-dense.cjs',36065),('class-length',s/'npm-class-function-length.cjs',None)]:
        outputs=[]
        for side,engine in [('node',node),('quench',cli)]:
            result=subprocess.run([engine,script],capture_output=True,timeout=120)
            Path(f'{prefix}-early-{label}-{side}.stdout').write_bytes(result.stdout)
            Path(f'{prefix}-early-{label}-{side}.stderr').write_bytes(result.stderr)
            assert result.returncode==0 and not result.stderr,(label,side,result.stderr[:300])
            outputs.append(result.stdout)
        assert outputs[0]==outputs[1],label
        data=json.loads(outputs[0]);assert data['complete']and(count is None or data['cases']==count)
        rows.append({'label':label,'complete':True,'cases':count})
    suite=Path('/workspace/scratch/quench-npm-file-url/tests/npm-top300')
    names=['punycode','tr46','json5','qs','ieee754','mime-types','string-width','json-buffer','minimist','path-key','eslint-visitor-keys','is-stream','statuses','whatwg-url','jackspeak','color-convert']
    for name in names:
        reports=[]
        for side,engine in [('node',node),('quench',cli)]:
            with tempfile.TemporaryDirectory()as cwd:
                result=subprocess.run([engine,suite/'scenario.mjs',name],cwd=cwd,capture_output=True,timeout=30)
                assert result.returncode==0 and result.stdout==f'npm-behavior-ok:{name}\n'.encode()and not result.stderr,(name,side,result.stderr[:400])
                reports.append(json.loads(Path(cwd,'function-checks.json').read_text()))
        assert reports[0]==reports[1],name
        rows.append({'package':name,'complete':True,'assertions':reports[0]['assertions'],'checks':reports[0]['checks'],'entry_modes':list(reports[0]['entries'])})
    Path(f'{prefix}-early-summary.json').write_text(json.dumps({'engines':engines,'rows':rows,'total_package_assertions':sum(r.get('assertions',0)for r in rows)},indent=2)+'\n')
except Exception as error:
    Path(f'{prefix}-early-error.txt').write_text(repr(error)+'\n');Path(f'{prefix}-early.exit').write_text('1\n');raise
else:Path(f'{prefix}-early.exit').write_text('0\n')
