# 51 — Legacy-free Value/object representation consolidation audit

Status: planned

The "no 15-year ABI legacy" advantage is only real if [[06-word-sized-value]], [[07-hidden-classes]], [[09-object-memory-model]], and [[11-string-representation]] actually land as a coherent, minimal representation rather than accumulating their own compatibility compromises over time (e.g. keeping the old `Value` enum path alive alongside a new word-sized path "just in case").

Concrete steps:
1. Once 06/07/09/11 individually reach `complete`, run a consolidation audit confirming no dead/parallel representation path remains in `main.rs`/`dynjit.rs` (e.g. no lingering `IndexMap<String, Value>` object path once hidden classes are the sole mechanism).
2. Verify the final representation was chosen for this VM's own semantics, not constrained by any external embedder-API compatibility requirement that doesn't actually apply here.
3. Record the final Value/object layout as a short design note so future contributors don't reintroduce a legacy-compatibility branch without deliberate justification.

Acceptance: exactly one object property representation and one Value representation exist in the codebase post-consolidation, with no parallel legacy path; the audit's design note is checked against the current source and matches it exactly (not aspirational); no correctness test relies on the old representation's specific behavior once removed.
