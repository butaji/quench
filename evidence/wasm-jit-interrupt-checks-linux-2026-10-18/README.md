# Compiled Wasm: interrupt checks on back edges and entries, through a poll page (2026-10-18)

Linux x86_64 cloud VM, 4 vCPU, 16 GB. Static production `quench-node --jit`, base `v2-merged`
`521732652`.

## What compiled code paid

An instruction-level Callgrind profile of the compiled CoreMark functions (the placed code
buffer disassembled and matched to per-address counts) put 84% of a fixed-work run in four
compiled functions, and 13% of their instructions in interrupt checks: every jump and branch,
forward ones included, loaded the flag's address from the call context (spilled, then
reloaded), loaded the flag atomically, compared and branched.

## Change

- Only an edge back to its own position or before, and each function entry, checks: a run that
  never stops passes one of them again and again (loops through the back edge, recursion
  without loops through the entry). The flag's address load is loop-invariant.
- Where every compiled entry runs through the fault-resuming trampoline (Linux x86-64), a
  check is one atomic read of the VM's poll page (`ExecutionInterrupt::poll_page`, one page
  mapped below 2 GiB on first use). A termination request makes the page unreadable; the
  SIGSEGV handler resumes a fault on the active call's poll page at a second trampoline
  landing that returns the terminated status, as the guard landing returns the
  out-of-bounds trap. Elsewhere the flag check stays.
- Accesses through one index share one wrapped-reservation check along straight-line code
  (the cache resets at every block a jump can enter).
- A plain (non-atomic) flag load was tried and rejected: Cranelift's alias analysis merged the
  loop's loads into one per call, so a loop would never see the request.

## Results

Fixed-work CoreMark (`run-fixed.mjs`: clock steps 1,1,1,1,2000 ms), Callgrind instructions:

| Build | Instructions |
| -- | --: |
| base | 6,382M |
| back edges and entries only | 5,714M |
| + loop-invariant address load | 5,402M |
| + shared wrapped-index checks | 5,390M |
| + poll page | 4,964M (−22%) |

`run-js.mjs` Score, interleaved single runs (noisy host): base 17.5–18.8K (median 17.9K),
back-edge checks 18.1–19.9K (median 19.2K), poll page 19.3–19.5K.

## Checks

Runtime lib 721/721, including `compiled_wasm_observes_termination_in_loops_and_loop_free_recursion`
(a spin loop and a loop-free recursive `fib(60)`, each terminated from another thread).
Wasm testsuite: no JIT 67,124/67,124; JIT 66,952/66,952 with `call_indirect.wast`'s worker
crashing on its stack as on the base (reported to the JIT lane). Guard: `callgrind-guard.txt`.

## After the merge with `v2-jit` (`v2-merged` `948f03f99`)

`v2-jit` landed backedge-only polling in parallel (`e955cff99`), routing each latch's taken
edge through a block of its own that holds the check. The merge kept that shape and measured
5,399M fixed-work instructions against this branch's 4,964M. Each incoming change was then
measured in isolation:

| Build | Instructions |
| -- | --: |
| this branch (`eeb00047e`) | 4,964M |
| + `ee75becfe` (wrapped addresses as i32) | 4,922M |
| + `f40e5dded`, `7961b7bde`, `280b9a011` | 4,923M |
| merge, as published | 5,399M |
| merge, without its `br_table` check | 5,399M |
| merge, check before the branch but the separate backedge block kept | 5,405M |
| merge, check before the branch, backedge direct | **4,885M** |

The separate block on the backedge, not the check, costs the 10%. That form also polled on
the forward edge of a `JumpI32Zero`, whose targets `branch` receives swapped: on the base
`521732652`, `e955cff99` alone measured 6,363M against 6,382M. `branch` again checks before
the transfer when either edge leads to `pc` or before, and leaves both edges direct.

A `br_table` now checks only when one of its entries lies at or before it (it checked on
every table before). The termination test gained a loop that repeats through a `br_table`
alone and a `br_if (i32.eqz ..)` loop (the swapped form); with either check removed, its
case runs past the termination request.

Checks: runtime lib 725/725; Wasm testsuite no JIT 65,188/65,188, JIT 65,016/65,016 with
`call_indirect.wast`'s worker failing as before (TOD-121). `run-js.mjs` Score, five interleaved
runs on a slower host window: this build 17.4–18.3K, `eeb00047e` 17.4–18.3K, Node 18.1–20.0K.
