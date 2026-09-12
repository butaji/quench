# 274 — Relocation-closed local-data stencil templates

Status: planned

Teach the stencil cooker to represent immutable local data and compiler-outlined helper
fragments without violating the copy-and-patch closure invariant. Task 273 found two
concrete rejected families: `-Os` emitted a local constant-pool reference to
`lCPI110_0`, while `-Oz` emitted cross-handler references to `OUTLINED_FUNCTION_5` and
`OUTLINED_FUNCTION_9`.

Keep one quoted representation: a template is a graph of code/data atoms plus typed
relocations, and copying/linking evaluates that graph once. A local constant-pool atom
may be copied with its owning stencil and patched by a symbolic internal relocation.
An outlined helper is admitted only if it is either a shared immutable `Kernel` or is
copied into the same closed `StencilInstance`; an unrepresented object-file dependency
remains a cooker error. Use named maximum atom, relocation, and alignment budgets.

Acceptance: `Os` and `Oz` catalogs either become fully relocation-closed or remain
explicitly rejected for a documented unsupported relocation; copied local data has
correct alignment/range; shared helpers are identity-deduplicated; 94 release tests
pass; the Task 273 matrix and full V8v7 A/B decide whether either pipeline replaces O2.

Depends on Tasks 02, 14, 16, 43, 52, 198, and 273.

Round-nineteen refinement: treat code and its immutable local data as one relocation
graph whose atoms may be laid out together in Task 189's near-code arena. Prefer
PC-relative edges for internal code, constants, and local tables, with explicit range
validation and counted island/absolute fallbacks. The final `StencilInstance` is closed
over all internal edges; a `Kernel` remains shared by identity and is reached through a
shared local island when direct reach is impossible. The R copy-and-patch implementation
reports meaningful runtime and code-size gains from this memory model, making it a
specific A/B candidate for the existing `-Os`/`-Oz` cooker matrix rather than a new
template representation:
<https://d3s.mff.cuni.cz/publications/kocourek_copyandpatch_2025/>.
