# 179 — Fast non-cryptographic hasher for compile-time tables

Status: planned

Every `HashMap` in this codebase (`src/main.rs`'s `ShapeRegistry::by_keys`/
`additions`/`removals`, `Env::names`, `block_kernel_entries`; `src/dynjit.rs`'s
`binding_layout`/`iterators`; `src/dynbytecode.rs`'s local-slot map) uses
`std::collections::HashMap`'s default hasher, SipHash — chosen by the standard
library for HashDoS resistance against untrusted network/user input, which does not
apply here: every key is a compiler-controlled identifier or shape-key vector derived
from the program's own source text during a single-process compile/link pipeline, not
attacker-supplied data crossing a trust boundary.

`rustc` made exactly this substitution for exactly this reason and measured it: the
switch from FNV to `FxHash` gave up to 6% compiler speedups, and reverting `FxHash`
back to the std default caused 4-84% slowdowns depending on the workload (see rustc-hash
source, linked below). Deegen's shape-key hashing ([[135-shape-hash-consing]],
[[150-cached-shape-transition-edges]]), binding-layout construction ([[13-environment-frames]]),
and block-kernel-entry lookups are structurally the same kind of small-key,
compiler-internal table lookup rustc optimized.

Replace `std::collections::HashMap`/`HashSet` with `rustc_hash::FxHashMap`/`FxHashSet`
(or an equivalent small dependency-free polynomial hasher) at every such site. This is a
drop-in hasher swap, not a data-structure change — it composes with, and does not block,
[[135]]'s shape interning or [[150]]'s transition-edge cache; those tasks' `HashMap`
tables get faster for free once this lands underneath them.

Acceptance: every non-adversarial-input `HashMap`/`HashSet` in the compile/link path
uses a fast non-cryptographic hasher; correctness tests pass unchanged (hasher choice is
not observable in program semantics); alternating A/B on compile-time-sensitive paths
(shape construction, binding layout, block-kernel lookup during linking) shows a
measured wall-clock improvement; no user-facing/network-facing input reaches an
FxHash-keyed table (verified by an audit of call sites, since FxHash is intentionally
not HashDoS-resistant).

Primary sources:
- rustc-hash (used by `rustc` itself, documents the FNV→FxHash and FxHash→default
  swap measurements): <https://github.com/rust-lang/rustc-hash>
- Hashing chapter, The Rust Performance Book: <https://nnethercote.github.io/perf-book/hashing.html>
