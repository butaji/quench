# peakbt.py THRESHOLD_KIB CMD... : stop the process when VmHWM passes the threshold, print stacks.
import subprocess, sys, time, os, re, signal
th = int(sys.argv[1]); p = subprocess.Popen(sys.argv[2:], stdout=subprocess.DEVNULL); fd = None; t0 = time.time()
while p.poll() is None:
    try:
        if fd is None: fd = os.open(f'/proc/{p.pid}/status', os.O_RDONLY)
        s = os.pread(fd, 4096, 0).decode(); h = int(re.search(r'VmHWM:\s+(\d+)', s).group(1))
        if h > th:
            os.kill(p.pid, signal.SIGSTOP)
            a = re.search(r'RssAnon:\s+(\d+)', s).group(1); f = re.search(r'RssFile:\s+(\d+)', s).group(1)
            print('stopped at', round((time.time()-t0)*1000, 1), 'ms hwm', h, 'anon', a, 'file', f)
            out = subprocess.run(['gdb', '-batch', '-p', str(p.pid), '-ex', 'thread apply all bt 25'], capture_output=True, text=True).stdout
            print('\n'.join(l for l in out.splitlines() if l.startswith('#') or l.startswith('Thread'))[:6000])
            os.kill(p.pid, signal.SIGKILL); break
    except Exception as e: pass
