#!/usr/bin/env python3
"""Instruction counts predicted without running the measured code or rebuilding.

Two static inputs, one per side of an interpreter step:
  * handler cost: every lane handler of the last built quench-node, disassembled. Each call-free
    path from the handler's entry to a dispatch (`jmp *`) is one way through it (one type case,
    one branch outcome); its instruction count is exact. `floor` is the shortest such path.
  * lane plan: which handler every bytecode instruction gets, from
    `QUENCH_TRACE=bytecode=lanes` (every function compiled and its lane view derived, nothing
    run when the script only defines functions). A run's plan also holds the handler each record
    settled on (requickening), which `--settled` uses.
A loop's predicted Ir/iter is the expected sum of its records' handler costs over one iteration
(conditional branches inside the loop at 1/2 each, the exit test never taken, fused records
covering the records they run). Runtime callees a handler calls (string building, allocation,
builtins) are outside the plan: an op whose work lives there is predicted as its lane part only.

usage:
  ir_predict.py handlers BINARY                    every handler: floor, path menu, symbol
  ir_predict.py ops BINARY PLAN [SETTLED]          ops.js loops: predicted vs ir_budget.txt
  ir_predict.py offfloor BINARY                    handlers measured above their floor, ranked
                                                   (ir_budget_handlers.txt), the excess per op

PLAN: QUENCH_TRACE=bytecode=lanes quench-node --no-jit load_only.cjs ops.js 2> PLAN
SETTLED: the same trace of a run (settle.cjs). Handler costs are cached beside the binary
(<binary>.handler-costs.tsv) until the binary changes.
"""
import bisect, collections, heapq, os, re, subprocess, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
OPS_JS = os.path.join(ROOT, 'quench-bench', 'primitives', 'ops.js')
BUDGET = os.path.join(ROOT, 'crates', 'quench-tier-corpus', 'ir_budget.txt')
SPLIT = os.path.join(ROOT, 'crates', 'quench-tier-corpus', 'ir_budget_handlers.txt')
HANDLER = re.compile(r'::lane_')
TRAP = ('panic', 'unwrap_failed', 'abort', 'unreachable', 'handle_alloc_error')
TARGET = re.compile(r'^([0-9a-f]+) <([^>]*)>')


def short_name(symbol):
    """`lane_operator<8>` as the ir-budget split names it, from a demangled symbol."""
    symbol = re.sub(r'quench_node::host::node_host::NodeHost|quench_runtime::test_host::\w+', 'H', symbol)
    head, _, generics = symbol.partition('::<')
    head = re.sub(r'^.*::', '', head)
    generics = ('<' + generics) if generics else ''
    return head + generics.replace('<H, ', '<').replace('<H>', '')


def symbols(binary):
    out = subprocess.run(['nm', '-C', '--defined-only', '-S', binary], capture_output=True, text=True).stdout
    found = []
    for line in out.splitlines():
        parts = line.split(' ', 3)
        if len(parts) == 4 and parts[2] in 'tTwW' and HANDLER.search(parts[3]):
            found.append((int(parts[0], 16), int(parts[1], 16), parts[3]))
    return sorted(found)


def disassemble(binary, syms):
    """Instructions per handler symbol, from one objdump call per cluster of nearby handlers."""
    starts = [address for address, _, _ in syms]
    code = collections.defaultdict(list)
    line_re = re.compile(r'^\s*([0-9a-f]+):\s+(\S+)\s*(.*)$')
    clusters, current = [], None
    for address, size, _ in syms:
        if current and address - current[1] < 4096:
            current[1] = max(current[1], address + size)
        else:
            current = [address, address + size]
            clusters.append(current)
    for low, high in clusters:
        out = subprocess.run(['objdump', '-d', '-C', '--no-show-raw-insn', '-w', f'--start-address={low:#x}',
                              f'--stop-address={high:#x}', binary], capture_output=True, text=True).stdout
        for line in out.splitlines():
            match = line_re.match(line)
            if not match:
                continue
            address = int(match.group(1), 16)
            index = bisect.bisect_right(starts, address) - 1
            if index >= 0 and address < syms[index][0] + syms[index][1]:
                code[syms[index][2]].append((address, match.group(2), match.group(3)))
    return code


def paths(code):
    """(sorted call-free dispatch path lengths, shortest path through calls) of one handler."""
    at = {address: index for index, (address, _, _) in enumerate(code)}

    def successors(index, through_calls):
        _, mnemonic, operands = code[index]
        if mnemonic.startswith('ret') or mnemonic == 'ud2' or mnemonic.startswith('int3'):
            return [], 'end'
        target = TARGET.match(operands)
        if mnemonic.startswith('jmp'):
            if '*' in operands or not target:
                return [], 'dispatch'
            address = int(target.group(1), 16)
            return ([at[address]], None) if address in at else ([], 'sibling')
        following = [index + 1] if index + 1 < len(code) else []
        if mnemonic.startswith('j') and target and int(target.group(1), 16) in at:
            return following + [at[int(target.group(1), 16)]], None
        if mnemonic.startswith('call'):
            callee = target.group(2) if target else '*'
            if any(word in callee for word in TRAP):
                return [], 'end'
            if not through_calls:
                return [], 'call'
        return following, None

    result = []
    for through_calls in (False, True):
        distance = [None] * len(code)
        distance[0] = 1
        queue, exits = [(1, 0)], {}
        while queue:
            length, index = heapq.heappop(queue)
            if length > distance[index]:
                continue
            following, exit_kind = successors(index, through_calls)
            if exit_kind == 'dispatch':
                exits.setdefault(index, length)
            for successor in following:
                if distance[successor] is None or length + 1 < distance[successor]:
                    distance[successor] = length + 1
                    heapq.heappush(queue, (length + 1, successor))
        result.append(sorted(exits.values()))
    return result[0], (result[1][0] if result[1] else None)


def handler_costs(binary):
    """{short name: (path menu, shortest path through calls)}, cached beside the binary."""
    cache = binary + '.handler-costs.tsv'
    if not os.path.exists(cache) or os.path.getmtime(cache) < os.path.getmtime(binary):
        syms = symbols(binary)
        code = disassemble(binary, syms)
        with open(cache, 'w') as out:
            for _, _, symbol in syms:
                if code.get(symbol):
                    menu, through = paths(code[symbol])
                    out.write(f"{','.join(map(str, menu)) or '-'}\t{through or '-'}\t{symbol}\n")
    costs = {}
    for line in open(cache):
        menu, through, symbol = line.rstrip('\n').split('\t')
        costs[short_name(symbol)] = ([int(x) for x in menu.split(',')] if menu != '-' else [],
                                     int(through) if through != '-' else None)
    return costs


def plan(path, binary):
    """({(program, function): {pc: record}}, {stub: chunk}) from a lane-plan trace."""
    names = {}
    for address, _, symbol in symbols(binary):
        names[address] = short_name(symbol)
    anchor = int(next(line.split()[1] for line in open(path) if line.startswith('LANEPLAN-ANCHOR')), 16)
    anchor_symbol = subprocess.run(['nm', binary], capture_output=True, text=True).stdout
    anchor_address = next(int(line.split()[0], 16) for line in anchor_symbol.splitlines() if 'lane_plan_anchor' in line)
    bias = anchor - anchor_address
    name = lambda word: names.get(int(word, 16) - bias, word)
    records, chunks = collections.defaultdict(dict), {}
    for line in open(path):
        if line.startswith('LANEPLAN-CHUNK'):
            _, program, function, chunk_program, chunk_function = line.split()
            chunks[(int(program), int(function))] = (int(chunk_program), int(chunk_function))
        elif line.startswith('LANEPLAN '):
            fields, text = line.rstrip('\n').split(' | ', 1)
            _, program, function, pc, width, handler, settled = fields.split()
            records[(int(program), int(function))][int(pc)] = {
                'width': int(width), 'handler': name(handler), 'settled': name(settled) if settled != '-' else None, 'text': text}
    return records, chunks


def jump_target(text):
    match = re.search(r'target=(\d+)', text)
    return int(match.group(1)) if match else None


LOOP_UPDATE = re.compile(r'_loop(<|$)|loop_kernel')
TEST_JUMP = re.compile(r'_jump(<|$)')


def loop_cost(body, handler_of, cost_of):
    """Expected Ir of one iteration of the function's last loop, and the handlers it calls out of."""
    backs = [pc for pc, r in body.items() if r['text'].startswith('Jump ') and (jump_target(r['text']) or 1 << 30) <= pc]
    if not backs:
        return None
    back = max(backs)
    head = jump_target(body[back]['text'])
    inside = lambda pc: head <= pc <= back
    fused = next((pc for pc in range(head, back) if pc + 1 == back and LOOP_UPDATE.search(handler_of(body[pc]))), None)
    start = head + 1 if fused is not None else head
    memo, calls_out = {}, set()

    def expected(pc):
        if not inside(pc) or pc not in body:
            return 0.0
        if pc in memo:
            return memo[pc]
        record = body[pc]
        handler, text = handler_of(record), record['text']
        cost, calls = cost_of(handler)
        if calls:
            calls_out.add(handler)
        target = jump_target(text)
        if pc == fused or (text.startswith('Jump ') and target is not None and target <= pc):
            following = []
        elif text.startswith('Jump ') and target is not None:
            following = [(1.0, target)]
        elif text.startswith('Jump') and target is not None:
            following = [(1.0, pc + record['width'])] if not inside(target) else [(0.5, pc + record['width']), (0.5, target)]
        elif TEST_JUMP.search(handler) and 'inc_dec' not in handler and pc + 1 in body and body[pc + 1]['text'].startswith('Jump') \
                and jump_target(body[pc + 1]['text']) is not None:
            # A test fused with the jump after it runs that jump itself.
            jump = jump_target(body[pc + 1]['text'])
            following = [(1.0, pc + 2)] if not inside(jump) else [(0.5, pc + 2), (0.5, jump)]
        else:
            following = [(1.0, pc + record['width'])]
        memo[pc] = cost + sum(p * expected(n) for p, n in following)
        return memo[pc]

    return expected(start), calls_out


def budgets():
    rows = {}
    for line in open(BUDGET):
        parts = line.split()
        if not line.startswith('#') and len(parts) >= 4:
            rows[parts[0]] = (float(parts[1]), float(parts[3]))
    return rows


def split_rows():
    for line in open(SPLIT):
        if not line.startswith('#'):
            op, value, handler = line.rstrip('\n').split(' ', 2)
            yield op, float(value), handler


def measured_units():
    """Each handler's most common whole Ir/iter in the split: its cost for one run, as measured."""
    values = collections.defaultdict(list)
    for _, value, handler in split_rows():
        if abs(value - round(value)) < 0.05:
            values[handler].append(round(value))
    return {handler: collections.Counter(v).most_common(1)[0][0] for handler, v in values.items()}


def op_functions(chunks):
    """ops.js's op names, each with its lane-plan function: the stubs of the evaluated program in
    source order, after the hoisted declarations."""
    source = open(OPS_JS).read()
    ops = re.findall(r'^ops\.(\w+) = function', source, re.M)
    declarations = len(re.findall(r'^function \w+', source, re.M))
    program = max(p for p, _ in chunks)
    stubs = sorted(k for k in chunks if k[0] == program)[declarations:]
    return list(zip(ops, stubs))


def command_ops(binary, plan_path, settled_path=None):
    costs = handler_costs(binary)
    units = measured_units()

    def static_cost(handler):
        menu, through = costs.get(handler, ([], None))
        return (menu[0], False) if menu else (through or 0, True)

    calibrated = lambda handler: (units[handler], False) if handler in units else static_cost(handler)
    records, chunks = plan(plan_path, binary)
    settled, settled_chunks = plan(settled_path, binary) if settled_path else ({}, {})
    rows, table = [], budgets()
    for op, stub in op_functions(chunks):
        body = records.get(chunks[stub])
        static = loop_cost(body, lambda r: r['handler'], static_cost) if body else None
        if not static:
            continue
        run = settled.get(settled_chunks.get(stub)) if settled else None
        settle = loop_cost(run, lambda r: r['settled'] or r['handler'], static_cost) if run else None
        both = loop_cost(run, lambda r: r['settled'] or r['handler'], calibrated) if run else None
        c, measured = table.get(op, (0.0, 0.0))
        rows.append((op, measured, c, static[0], settle[0] if settle else None, both[0] if both else None, sorted(static[1])))
    print(f"{'op':16}{'measured':>9}{'C':>7}{'static':>8}{'settled':>9}{'calib':>8}  static plan handlers that call out")
    for op, measured, c, static, settle, both, calls in rows:
        show = lambda v: f"{v:8.1f}" if v is not None else '       -'
        print(f"{op:16}{measured:9.1f}{c:7.1f}{static:8.1f} {show(settle)}{show(both)}  {','.join(calls)}")
    for column, label in ((3, 'static plan x static cost'), (4, 'settled plan x static cost'), (5, 'settled plan x measured cost')):
        errors = sorted(abs(r[column] - r[1]) / r[1] for r in rows if r[1] and r[column] is not None)
        if errors:
            share = lambda x: 100 * sum(e <= x for e in errors) / len(errors)
            print(f"# {label}: {len(errors)} ops, median error {100 * errors[len(errors) // 2]:.0f}%, "
                  f"within 10% {share(0.1):.0f}%, within 25% {share(0.25):.0f}%")


def command_handlers(binary):
    for handler, (menu, through) in sorted(handler_costs(binary).items()):
        print(f"{menu[0] if menu else '-':>5}  {handler}  [{','.join(map(str, menu[:12]))}]  through-calls={through}")


def command_offfloor(binary):
    costs = handler_costs(binary)
    rows = collections.defaultdict(lambda: [0.0, [], 0, []])
    menu_hits = collections.Counter()
    for op, value, handler in split_rows():
        menu = costs.get(handler, ([], None))[0]
        if not handler.startswith('lane_') or not menu:
            continue
        floor = menu[0]
        runs = max(1, round(value / floor))
        runs = runs if abs(value - runs * floor) < 0.5 else 1
        menu_hits['in path menu' if round(value) in menu or abs(value - runs * floor) < 0.5 else 'outside menu'] += 1
        if value - runs * floor > 0.5:
            row = rows[handler]
            row[0] += value - runs * floor
            row[1].append(f"{op}:{value:.0f}")
            row[2], row[3] = floor, menu
    print(f"# measured handler costs vs static path menus: {dict(menu_hits)}")
    print(f"{'excess':>7} {'floor':>5}  handler  [path menu]  ops:measured Ir/iter")
    for handler, (excess, ops, floor, menu) in sorted(rows.items(), key=lambda kv: -kv[1][0]):
        print(f"{excess:7.0f} {floor:5}  {handler}  [{','.join(map(str, menu[:10]))}]  {' '.join(ops[:8])}")


if __name__ == '__main__':
    commands = {'handlers': command_handlers, 'ops': command_ops, 'offfloor': command_offfloor}
    if len(sys.argv) < 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]](*sys.argv[2:])
