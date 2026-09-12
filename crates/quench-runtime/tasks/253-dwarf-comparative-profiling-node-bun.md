# 253 — DWARF-based comparative native profiling: deegen vs. Node (V8) vs. Bun (JSC)

Status: complete

`scripts/run-v8v7.sh:67` already invokes macOS's `/usr/bin/sample` — a DWARF/frame-
pointer-based native sampling profiler — against this project's own binary to produce
the `.sample.txt` artifacts cited throughout the research rounds (e.g.
`reports/task161-richards-accepted.sample.txt`). Node and Bun are both installed
locally (`node v26.7.0`, `bun 1.3.14`, confirmed present). The same tool can sample all
three processes, but the experiment below disproved the stronger original claim that
the resulting stacks would be directly comparable: Node and Bun expose idle runtime
threads and mostly-unsymbolized generated-code addresses to macOS `sample`. V8's own
`--prof` output is required to attribute generated JavaScript code and compilation tier.

This closes a real gap: every existing research round (143, 151, 161, 170, 182, 201,
215, 226, 228) reasons about V8/JSC's *design* from blog posts and papers, never from a
direct, symbolized measurement of what those engines' compiled code actually spends time
on for *this specific corpus*. A profile is a different, more concrete kind of evidence
than a design-document citation — it tells you which builtins/stubs actually dominate
for *this* input, not which mechanisms the engine's authors describe as important in
general.

Concrete steps:
1. Run each V8v7 suite under `node` (default V8 execution) and `bun` (default JSC/Bun
   execution) with `sample` attached the same way `run-v8v7.sh` already attaches it to
   this project's own binary, producing one `.sample.txt` per suite per engine.
2. For V8 specifically, additionally capture `node --prof`/`--prof-process` output —
   V8's own built-in profiler, which (unlike generic native sampling) annotates each
   hot JS function with its compilation tier (interpreted/Sparkplug/Maglev/Turbofan,
   shown via V8's `*`/`~` prefix convention) — giving a second, richer signal beyond
   what frame-pointer sampling alone reveals about *which tier* is actually running the
   hot code for each suite.
3. Normalize all three profiles (deegen, Node, Bun) into the same top-N-by-suite format
   this project's own `reports/*-summary.txt` artifacts already use (per [[132]]'s
   residual-block-profile precedent), so they are mechanically comparable rather than
   three differently-shaped reports someone has to reconcile by eye.
4. Do not draw architectural conclusions in this task — it produces the raw comparative
   evidence; [[254]] is where that evidence gets interpreted against V8/JSC source.

Acceptance: one `.sample.txt` (or equivalent) exists per suite for each of deegen, Node,
and Bun, stored under `reports/` following this project's existing naming convention;
V8's `--prof-process` tier-annotated output exists for at least the suites where this
project's own score is furthest from target (per [[15]]'s ledger) since those are where
the comparison is most informative; all three profiles are normalized into one
comparable top-N format; this task's own acceptance is the produced artifact set, not
any interpretation of it.

Tooling note: `perf` (Linux) is unavailable on this Darwin/arm64 host; `sample` and
`dtrace`'s `profile`/`pid` providers (both present, confirmed via `xcrun -f dtrace`) are
the macOS-native equivalents this task uses instead — consistent with this project's
own existing tooling choice, not a new dependency.

## Result

Complete, with the direct-comparability premise corrected rather than preserved.

- `scripts/v8v7-shell-adapter.cjs` supplies the benchmark shell's `load` and `print`
  operations under Node and Bun.
- `scripts/profile-comparative-v8v7.sh` records all eight suites under deegen, Node,
  and Bun, plus V8's tier-aware profiler for every Node run.
- `scripts/normalize-macos-sample.sh` and `scripts/normalize-v8-prof.sh` produce the
  normalized native and V8-JavaScript tables.
- `reports/task253-comparative/` contains 24 native sample reports, eight V8 logs,
  eight processed V8 reports, `native-top.tsv`, `v8-js-top.tsv`, `scores.txt`, and
  `versions.tsv`.

The useful differential is unambiguous even though JIT-frame symbolization differs.
Deegen's generic `dyn_block_step_impl` alone accounts for 29.23% of Richards, 26.85%
of DeltaBlue, 18.53% of Crypto, 26.84% of RayTrace, 26.22% of Earley-Boyer, and 18.48%
of Splay samples. Frame completion, frame creation, and `Value` destruction add another
large visible share in the object/call-heavy suites. By contrast, V8's tier profiler
attributes the dominant work to optimized (`*`) JavaScript functions: for example
Richards' scheduler/task bodies, Crypto's bignum loops, and Navier-Stokes' `project`.

Raw native percentages must not be compared across engines as if symbol coverage were
equal. `native-top.tsv` is authoritative for named host/runtime frames; `v8-js-top.tsv`
is authoritative for V8 generated JavaScript. Bun's generated JSC frames remain mostly
unknown addresses, which is an explicit limitation for Task 254 rather than silently
invented symbol attribution.
