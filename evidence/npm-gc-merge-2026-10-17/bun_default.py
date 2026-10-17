import os, sys, subprocess, tempfile
def peak(argv, env, package):
    with tempfile.TemporaryDirectory() as d:
        p = subprocess.Popen(argv + ['/home/user/quench/tests/npm-top300/scenario.mjs', package], cwd=d, env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        out = p.stdout.read(); _, status, usage = os.wait4(p.pid, 0)
        return usage.ru_maxrss / 1024, status == 0 and f'npm-behavior-ok:{package}'.encode() in out
smol = dict(os.environ); default = {k: v for k, v in os.environ.items() if k != 'BUN_OPTIONS'}
print(f"{'package':40s} {'quench':>8s} {'bun --smol':>11s} {'bun default':>12s}")
for name in open(sys.argv[1]).read().split():
    q = peak(['/root/runs/quench-m4'], smol, name); s = peak(['/root/.bun/bin/bun'], smol, name); b = peak(['/root/.bun/bin/bun'], default, name)
    print(f"{name:40s} {q[0]:8.1f}{'' if q[1] else '!'} {s[0]:11.1f}{'' if s[1] else '!'} {b[0]:12.1f}{'' if b[1] else '!'}", flush=True)
