# NavierStokes `--jit`: where native loop time goes (Linux, 2026-10-24)

Diagnostic evidence for the JIT owners (TOD-110, TOD-186 and TOD-138). Lane
`v2-cloud-c` writes no JIT code under decision 35, so this records measurements
only. Source: `v2-cloud-c` at `e1cf92e0c0`.

Host: a 4-vCPU Intel Xeon @ 2.10 GHz VM (Sapphire Rapids) with 15 GiB RAM and no
PMU counters. Concurrent runs are pinned to CPUs 1 and 2.

## JIT-mode screen (3 concurrent pairs per fixture, `jit-board-3pairs.txt`)

The ratio is the reference Score divided by Quench `--jit`'s Score, so a value
above 1 means the reference is ahead.

| Fixture | Node (JIT) | Bun (JIT) |
|---|---:|---:|
| Richards | 19.1 | 21.9 |
| DeltaBlue | 47.5 | 38.3 |
| Crypto | 7.43 | 8.36 |
| RayTrace | 15.4 | 21.2 |
| EarleyBoyer | 7.05 | 6.26 |
| RegExp | 1.59 | 4.14 |
| Splay | 0.76 | 2.41 |
| NavierStokes | 1.41 | 1.08 |

With 3 pairs this is a screen, not a claim. Quench `--jit` runs NavierStokes 11.7×
and Crypto 2.0× faster than Quench without `--jit` (3 pairs each).

## Instruction split (callgrind, one `mat-navier-stokes.js` run)

- Total: 2.45 G instructions; about 70% are in JIT mappings.
- One mapping, the `lin_solve` OSR region (10,320 bytes), takes **43%** of the
  total.
- The `advect` region (24,472 bytes) takes **11%**.
- `QUENCH_JS_JIT_STATS`: every hot loop compiles, with no entry misses, exits or
  widenings.

Method: a temporary local hook (not committed) wrote each compiled body's bytes
and CLIF and logged its mapping address. `jitinstrs.py` attaches callgrind
`--dump-instr=yes` costs to an `objdump` listing of the body.

## `lin_solve` inner loop: 48 instructions per element (`lin_solve-inner-loop.txt`)

The loop is
`lastX = x[currentRow] = (x0[currentRow] + a*(lastX+x[++currentRow]+x[++lastRow]+x[++nextRow])) * invC`.
The counted-loop proof already removes its bounds checks. The remaining work per
element:

1. **Constant key offsets do not fold.** Each `x[++r]` emits `lea r, [r+1]` and
   then `movsxd`, instead of using a `+8` displacement on the shared root.
   - In the CLIF the key is `ireduce.i32(uextend.i64(iadd r, 1))`, because
     `Body::write` keeps the i32 view in an i64 variable.
   - `Lowering::int32_root` stops at the `ireduce`, so `ElementIndex.displacement`
     stays 0.
   - Cranelift removes the extend pair afterwards, but the separate address
     arithmetic remains.
   - Estimated saving: about 6 instructions per element. Looking through
     `ireduce(uextend(x))` in `int32_root` and `int32_constant` should be enough.
2. **The x0 base is reloaded from a stack spill** (`mov r9, [rsp+0x50]`) on every
   element.
3. **Every element load has a NaN check** (`vucomisd` and `jp`), because integral
   Numbers are stored with the integer tag. This is required while arrays have no
   all-double element kind.
4. **Every element store checks the old word for a hole**: a load, a 64-bit mask
   and a compare against a RIP-relative constant (4 instructions).
5. **Boxing tests each stored double for an integral value** (`vroundsd`,
   `vucomisd`, `je`). This is required by the one-Number-representation encoding.
6. **The poll page is read on each back edge** (2 instructions), and the block
   layout leaves a `mov` and a `jmp` on the hot path.

Fixing items 1, 2 and 6 brings the loop to about 38 instructions, roughly 1.26×
on this region. That is not enough for a 1.2× win over Node, which needs about
1.7×. Items 3–5 come from the boxed element representation. Removing them needs
an array element kind that stores raw doubles, which is a runtime representation
decision, not an emitter fix.

## `advect` (`region-021-hot.txt`)

This loop has no counted-loop proof, so every access keeps its bounds check.
Each access also reloads its array base and its length from stack spills
(`[rsp+0x30]` through `[rsp+0x48]`). The region pins one base and one length per
array as values live across the loop, which exceeds the register budget.

## Per-function time and the `lin_solve` floor (`ns-timed.js`)

`ns-timed.js` wraps each solver function to record exclusive time. One run per
engine on CPU 1:

| ms per call, exclusive | Node | Bun | Quench `--jit` |
|---|---:|---:|---:|
| `lin_solve` | 1.44 | 1.40 | **1.23** |
| `advect` | 0.172 | 0.164 | 0.282 |
| `project` (excluding inner calls) | 0.193 | 0.172 | 0.265 |
| `set_bnd` | 0.0020 | 0.0009 | 0.0045 |

`lin_solve` is 64% of Quench's time, and Quench is already the fastest engine on
it. All three engines are bound by the same serial floating-point chain.
`lastX` feeds `((lastX + p) + q) + r`, then `*a`, `+x0` and `*invC`, and IEEE
order forbids reassociating it. The `linsolve-fixed.js` kernel confirms this:
Node 428 ms, Bun 458 ms, Quench 460 ms.

Folding constant key offsets into the displacement (item 1 above) removes 3
instructions per element. It measured 3.4% slower on that kernel, so it was not
kept.

What this means for a 1.2× win:
- **vs Bun:** with `lin_solve` at 1.23 ms per call, everything else must take at
  most 0.23 ms per `lin_solve` call. Bun spends 0.36 ms on it, so Quench would
  have to be about 1.5× faster than JavaScriptCore's optimizing tier there.
- **vs Node:** everything else must take at most 0.34 ms per call, against
  Node's 0.44 ms.
- **At parity:** matching both engines on everything except `lin_solve` gives
  only about 1.1×.

## Shared guarded arithmetic (`int32_key`)

In `set_bnd`, `j*rowSize` was multiplied four times per iteration, each copy with
its own overflow and −0 guards. The exact-arithmetic cache keys on operand
values. Each read of an `i32` register view emits a fresh `ireduce`, and a view
written in the same chain reads back as `ireduce(uextend(x))`, so the keys never
matched. Keying on the SSA value each operand is a pure function of lets later
copies in the chain reuse the guarded result.

Results:
- `setbnd-fixed.js`, fixed work: 18,750 → 17,400 instructions per call (−7%).
  Wall time +2.6% (11 pairs, 6/11).
- `lin_solve` kernel: neutral.
- Whole fixtures: within this host's JIT-mode noise. NavierStokes measured +9%
  over 7 pairs and −1.5% over 11 pairs.
- Runtime lib tests: 918/918 pass.
- `--jit` oracles: 137 of 138 match. The exception, `deeprec`, also fails on the
  baseline: deep recursion throws `RangeError: Maximum call stack size exceeded`
  under `--jit` and completes under Node.
