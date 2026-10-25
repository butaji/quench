# margin.py BOARD.json... : per cell, target = 0.8 x best other min; margin = target - Quench median.
import json, sys
MARGIN_REQUIRED = 60
for path in sys.argv[1:]:
    d = json.load(open(path)); fails = 0; print(path)
    for name, fx in d['fixtures'].items():
        others = [r['min'] for r in fx['rows'] if r['side'] == 'other' and r['valid']]
        target = int(0.8 * min(others))
        for r in fx['rows']:
            if r['side'] != 'quench': continue
            m = target - r['median']; ok = 'PASS' if m >= 0 else 'FAIL'; fails += m < 0
            flag = '' if m >= MARGIN_REQUIRED or m < 0 else ' (thin)'
            w = target - r['max']; wok = 'ok' if w >= 0 else 'WORST-FAIL'
            print(f"  {name:14} {r['engine']:13} median {r['median']:6} worst {r['max']:6} target {target:6} margin {m:6} {ok}{flag}  worst-margin {w:6} {wok}")
    print('  fails', fails)
