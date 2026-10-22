import json,sys
d=json.load(open(sys.argv[1]))
print(f"{'fixture':14}{'q':>7}{'q-jit':>7}{'qjs':>7}{'target':>7}{'ratio':>7}")
for fx,f in d['fixtures'].items():
    r={x['engine']:x for x in f['rows']}
    qw=max(r['quench']['max'],r['quench --jit']['max'])
    ob=min(x['min'] for x in f['rows'] if x['side']=='other' and x.get('valid'))
    print(f"{fx:14}{r['quench']['max']:7}{r['quench --jit']['max']:7}{ob:7}{int(ob*0.8):7}{qw/ob:7.3f}{'  OK' if qw<=ob*0.8 else ''}")
