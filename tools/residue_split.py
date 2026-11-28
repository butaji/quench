#!/usr/bin/env python3
"""Fixed-work residue split of a callgrind pair, for AOT image code and for C.

Usage:
  residue_split.py image SHORT.cg LONG.cg RUNS DUMP_DIR DUMP_STDERR
  residue_split.py c     SHORT.cg LONG.cg RUNS BINARY

SHORT/LONG are `valgrind --tool=callgrind --dump-instr=yes` outputs of the same program
doing N and N+RUNS units of work; their difference over RUNS is the per-unit cost with
startup removed. `image` maps per-instruction cost onto the code an AOT image or JIT
dumped with QUENCH_TRACE=jit-dump=DUMP_DIR (run that dump under `valgrind --tool=none`
so the addresses match callgrind's, and keep its stderr). `c` maps it onto a native
binary. Both print the same instruction-class table, so the two are comparable, plus the
never-taken conditional branches ("checks in execution") and the instructions feeding them.
"""
import collections
import re
import subprocess
import sys


def costs(path):
    """Per-instruction self cost from a callgrind file written with --dump-instr=yes."""
    cost = collections.Counter()
    addr = 0
    after_calls = False
    for line in open(path):
        if line.startswith('calls='):
            after_calls = True
            continue
        if line[:1] not in '0+-*':
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        pos = parts[0]
        if pos.startswith('0x'):
            addr = int(pos, 16)
        elif pos[0] in '+-':
            step = int(pos[1:], 0)
            addr += step if pos[0] == '+' else -step
        if after_calls:  # the line after calls= is the call's inclusive cost
            after_calls = False
            continue
        cost[addr] += int(parts[2])
    return cost


def per_unit(short, long_, runs):
    a, b = costs(short), costs(long_)
    return {k: (b.get(k, 0) - a.get(k, 0)) / runs for k in set(a) | set(b)}


INSN = re.compile(r'\s*([0-9a-f]+):\s+(?:[0-9a-f]{2} )*\s*([a-z]\S*)\s*(.*)')
TAG = re.compile(r'0x7ff[0-9a-f]{13}|\$0x30,|\$0x2f,|\$0x2c,|0x7ffc8|\$0x7ff[0-9a-f]\b')
RECORD = re.compile(r'0x7000000[0-9a-f]|\$0xc,|0x3fffffff|0x1fffffff|0xc7ffffff|0xfffffff\b|\(%r13,%r\w+,8\)')


def classify(op, args):
    if op.startswith(('call', 'ret', 'push', 'pop')):
        return 'call/ret'
    if TAG.search(args):
        return 'tag/box'
    if RECORD.search(args):
        return 'record/layout'
    if '(%rsp' in args or '(%rbp' in args:
        return 'stack spill/reload/frame'
    if op.startswith(('set', 'cmov', 'movzb')):
        return 'bool materialize'
    if op.startswith('j'):
        return 'branch'
    if op.startswith(('cmp', 'test', 'ucomi')):
        return 'compare'
    if op.startswith('movabs'):
        return 'constant'
    if op.startswith('mov') and '(' in args:
        return 'load' if '(' in args.split(',')[0] else 'store'
    if op.startswith(('mov', 'lea')):
        return 'move/lea'
    return 'alu'


def disassemble(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True, check=True).stdout
    rows = []
    for line in out.splitlines():
        m = INSN.match(line)
        if m:
            rows.append((int(m.group(1), 16), m.group(2), m.group(3)))
    return rows


def report(regions, cost):
    classes = collections.Counter()
    rest = collections.Counter()
    branches = never_taken = slice_cost = 0.0
    for rows in regions:
        checks = set()
        for i, (a, op, args) in enumerate(rows):
            n = cost.get(a, 0)
            if n <= 0 or not op.startswith('j') or op == 'jmp':
                continue
            branches += n
            target = re.match(r'0x([0-9a-f]+)|([0-9a-f]+)\b', args)
            if not target or cost.get(int(target.group(1) or target.group(2), 16), 0) != 0:
                continue
            never_taken += n
            checks.add(a)
            k = i
            while k > 0 and i - k < 8:  # the instructions feeding the check
                k -= 1
                pa, pop, pargs = rows[k]
                if pop.startswith(('j', 'call', 'ret')) or re.search(r',\s*-?0x[0-9a-f]*\(%r[sb]p', pargs):
                    break
                slice_cost += cost.get(pa, 0)
                checks.add(pa)
        for a, op, args in rows:
            n = cost.get(a, 0)
            if n > 0:
                c = classify(op, args)
                classes[c] += n
                if a not in checks:
                    rest[c] += n
    total = sum(classes.values())
    print('mapped Ir per unit %.0f' % total)
    for k, v in classes.most_common():
        print('  %-26s %11.0f %5.1f%%' % (k, v, 100 * v / total))
    print('conditional branches %.0f, never taken (checks) %.0f' % (branches, never_taken))
    checks_total = never_taken + slice_cost
    print('checks in execution incl. feeding instructions %.0f (%.1f%%)' % (checks_total, 100 * checks_total / total))
    print('outside checks:')
    for k, v in rest.most_common():
        print('  %-26s %11.0f' % (k, v))


def main():
    mode, short, long_, runs = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
    cost = per_unit(short, long_, runs)
    print('per-unit Ir (all code) %.0f' % sum(cost.values()))
    if mode == 'image':
        directory, stderr = sys.argv[5], sys.argv[6]
        regions = []
        for line in open(stderr):
            m = re.search(r'(code-\d+) .*address=(0x[0-9a-f]+) bytes=\d+', line)
            if m:
                regions.append(disassemble(['objdump', '-D', '-b', 'binary', '-mi386:x86-64',
                                            '--adjust-vma=' + m.group(2), '%s/%s.bin' % (directory, m.group(1))]))
    else:
        regions = [disassemble(['objdump', '-d', '--no-show-raw-insn', sys.argv[5]])]
    report(regions, cost)


if __name__ == '__main__':
    main()
