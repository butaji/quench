# 197 — Layered differential performance testing

Status: planned

Extend the optimization routine with generated small JS programs exercised under paired
configurations: generic versus direct recipes, one rewrite disabled versus enabled, and
previous accepted binary versus candidate. Repeatedly retain only cases whose timing or
dynamic-operation ratio exceeds named thresholds, then minimize and deduplicate them by
normalized bytecode/stencil shape.

This is test infrastructure, never runtime hotness. Programs and measurements are event
data; prioritization, filtering, and clusters are derived views. Functional output must
match before any performance divergence is considered.

Acceptance: the tool rediscovers at least one known accepted win and one known rejected
regression from Tasks 94–178; alternating order/noise filtering avoids false positives;
every retained case records binary hashes and active feature flags; minimized cases link
back to a normalized recipe rather than a benchmark filename.

Primary source: Jittery's layered differential performance testing found 12 previously
unknown JIT performance bugs, <https://arxiv.org/abs/2603.06551>.

## Round-nineteen refinement: prove the machine bottleneck moved

For every candidate expected to alter native execution, add a diagnostic measurement
layer using the host's CPU Counters, CPU Profiler, and disassembly. Record named metrics
when the platform exposes them: retired instructions, cycles, conditional branches,
branch misses, L1 instruction/data misses, native code bytes, helper/kernel transfers,
and the longest visible load-to-use dependency chain. Missing counters are recorded as
unavailable, never synthesized. A repeated alternating end-to-end A/B remains the only
acceptance signal; counters explain a result but cannot override it.

This rule responds directly to the 2025 negative inline-cache result: patching cached
offsets into machine instructions removed one or two loads yet did not improve runtime,
and small unrelated layout changes moved results by roughly five percent. Therefore
"fewer loads", "fewer stencils", or "smaller disassembly" alone is no longer an
optimization claim. Use enough randomized alternating samples for the existing noise
gate and retain binary hashes/layout metadata with every counter capture:
<https://arxiv.org/pdf/2502.20547>.
