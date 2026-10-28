import os,random,re,sys
root='/home/user/quench/tests/test262'
out=sys.argv[1]
dirs=['test/language/expressions','test/language/statements','test/language/function-code']+['test/built-ins/'+d for d in ['Array','String','Object','Function','Math','JSON','Number']]
harness=open(root+'/harness/assert.js').read()+'\n'+open(root+'/harness/sta.js').read()+'\n'
n=0
for d in dirs:
    files=[]
    for dp,_,fs in os.walk(os.path.join(root,d)):
        for f in fs:
            if f.endswith('.js') and not f.endswith('_FIXTURE.js'): files.append(os.path.join(dp,f))
    files.sort()
    random.seed(13); random.shuffle(files)
    picked=0
    for f in files:
        s=open(f,encoding='utf-8',errors='replace').read()
        m=re.search(r'/\*---(.*?)---\*/',s,re.S)
        meta=m.group(1) if m else ''
        if 'includes:' in meta or 'negative:' in meta: continue
        flags=re.search(r'flags:\s*\[(.*?)\]',meta)
        if flags and any(x in flags.group(1) for x in ['module','async','raw','onlyStrict','CanBlock']): continue
        open(os.path.join(out,'%03d-%s'%(n,os.path.basename(f))),'w').write(harness+s)
        n+=1; picked+=1
        if picked==30: break
print(n,'files')
