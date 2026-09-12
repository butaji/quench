# 336 — Reusable monomorphic call-site activations

Status: complete

Cache one cleared `DynFrame` activation at each monomorphic `CallIcSite` and reuse it for
every non-capturing invocation of the site's immutable `FunctionCallRecipe`. This removes
repeated allocation and construction of the complete Rust sidecar and owned value buffer
while preserving the existing stencil-only entry and exact JS call semantics.

This is the mutable activation half of [[317-immutable-call-recipes-and-inline-targets]].
The recipe, entry image, cooked kernels, and stencil templates remain immutable and shared.
The cached activation is not executable code and is never shared concurrently: the site
takes it before entering the callee and returns it only after completion. Consequently a
recursive or re-entrant call through the same site sees an empty cache, allocates a
temporary activation, and remains correct. Capturing callees retain the canonical path.

`DynJitCode::call_with_reusable_activation` constructs the canonical frame once, then
reinitializes its derived binding range on later entries. Completion preserves active-frame
rooting, source identity, exception/result ownership, and GC behavior before clearing every
owned value, iterator, handler, pending-throw, region-view, and continuation field. An
inactive cached activation therefore contains no untraced guest values. Exact layout,
environment, and code-owner assertions prevent applying an instance to an incompatible
recipe.

The same mechanism is wired into both the ordinary monomorphic call-IC edge and the
experimental direct-call-region edge. Named runtime counters expose allocations and
reuses; no execution-count threshold or benchmark-specific selector was introduced.

## Validation

- `cargo test --release`: 124 passed.
- `DEEGEN_OBJECT_GC_STRESS=1 cargo test --release --quiet`: 124 passed.
- `cargo fmt -- --check`: passed.
- Unit coverage proves that two calls return the same cleared activation address.
- Richards dynamic wiring proof over a 300 ms window records 68 activation allocations
  and 5,291,282 activation reuses.
- The five-pair call-heavy comparison in
  `reports/task336-reusable-call-activation-targeted-ab-5/comparison.txt` improves the
  geometric aggregate 1406.51 -> 1498.68 (+6.55%).
- The three-pair complete-suite comparison in
  `reports/task336-reusable-call-activation-full-ab-3/comparison.txt` improves the exact
  baseline 2117.62 -> **2198.90 (+3.84%)**. Richards is +9.69%, DeltaBlue +17.26%, Crypto
  +4.08%, RayTrace +4.65%, Earley-Boyer +1.28%, RegExp -0.80%, Splay -3.07%, and
  Navier-Stokes -0.92%; every component clears the standing -5% floor.
- Baseline `/tmp/deegen-task332-site-stride-v2` SHA-256:
  `7877b5fc01f4ceeb206e1c0e03658b9caff957beb40b7287b67408d82258d9ce`.
- Candidate `/tmp/deegen-task336-call-activation-cache` SHA-256:
  `453322a3a669547a49d5fd5851363954cde7937d3da14a6224be080b8d666bde`.

This result validates boundary-sized work but does not complete [[146-direct-continuation-vm-call-stack]]
or [[181-native-direct-user-call-continuation-regions]]. Warm calls still enter and leave
through Rust helpers; the next call slice must erase that boundary through native
continuations or quote-level inlining, not add another sidecar cache.
