# 153 — Quoted IC recipe algebra

Status: planned

Replace hand-authored IC fast-path combinations with one immutable, typed recipe:

`IcExpr = Guard* ; Pure* ; Result`

Guards refine categorical contexts, pure/idempotent nodes may only read state, and the
single terminal result node owns any observable effect. Inputs, per-instance fields
(shape, slot, callee, atom), and build-time immediates are distinct data classes. Recipe
composition is a normalized free monoid; lowering to a `Kernel` or patched
`StencilInstance` is a functor over that same quote.

Rust macros define the finite operation vocabulary once and generate validation,
metadata layout, rustc/LLVM AOT handler declarations, patch descriptions, diagnostics,
and tests. The runtime does not interpret this IR: it selects a closed recipe, copies or
references its cooked code, patches fields, and executes it.

Match SpiderMonkey's effect discipline exactly in the first normal form: zero or more
guards, then zero or more idempotent side-effect-free operations, then exactly one
result operation in terminal position. That makes slow re-entry safe by construction:
a failed guard cannot occur after an observable mutation, and recipe fusion cannot move
a second effect ahead of the canonical result. The recipe remains quoted build/link
data and must never become a second runtime interpreter.

Acceptance: own-property get/set and direct user call are expressed without adding new
branches to the selector; invalid guard/effect ordering is rejected structurally; the
same recipe structure shares a kernel/template across different fields; all existing IC
semantics and the full V8v7 gate pass.

Checked against CPython's PEP 659 specializing adaptive interpreter, which organizes the
same idea as a "family" of adaptive/specialized instruction forms per opcode
(`LOAD_ATTR`, `BINARY_ADD`, etc.) selected by a per-site operand-kind guard — structurally
the same guard-then-specialized-form shape as this recipe algebra. The one piece of
PEP 659 deliberately not adopted is its counter-gated "quickening": an adaptive
instruction counts down and only specializes after a threshold of executions. This VM's
guard/recipe compiles its specialized form on first use, with no counter and no
per-instruction hotness state, matching this project's standing no-tier/no-hotness-
threshold constraint (see [[45-no-tier-first-execution]]).

Primary sources: <https://firefox-source-docs.mozilla.org/js/cacheir.html>;
CPython PEP 659: <https://peps.python.org/pep-0659/>.

## Shared-code refinement

Make recipe identity depend only on opcode sequence and operand classes, not concrete
shape, property-slot, atom, or callee values. Those concrete fields live in immutable
per-site data read by one shared Kernel, or become copy patches only when measurement
proves baking them into a StencilInstance wins enough to justify code growth. This is
CacheIR's baseline split: one code body can serve many stub-field payloads.

The same quote also feeds later optimized-region lowering. There is no second IC
semantic definition: `IcExpr -> shared Kernel` and `IcExpr -> in-region stencils` are
two structure-preserving lowerings from the one validated recipe.
