import os, sys, subprocess, tempfile
def peak(binary, package):
    with tempfile.TemporaryDirectory() as d:
        p = subprocess.Popen([binary, '/home/user/quench/tests/npm-top300/scenario.mjs', package], cwd=d, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        out = p.stdout.read()
        _, status, usage = os.wait4(p.pid, 0)
        ok = status == 0 and f'npm-behavior-ok:{package}'.encode() in out
        return usage.ru_maxrss / 1024, ok
for line in open(sys.argv[1]):
    name, best = line.split()
    row = [peak(b, name) for b in sys.argv[2:]]
    print(f"{name:40s} best-other {int(best)/1024:7.1f} " + " ".join(f"{m:7.1f}{'' if ok else '!'}" for m, ok in row), flush=True)
