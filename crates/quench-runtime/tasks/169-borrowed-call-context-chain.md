# 169 — Borrowed call context chain

Status: complete

Make the lexical context of an executing stencil frame a call-scoped borrowed capability
instead of an owned `Rc<RefCell<Environment>>`. `DynFrame` stores a pointer to the
caller-owned `Env`; semantic kernels recover `&Env` through one helper, and only
operations that semantically capture the environment clone it.

Derive the bounded inline environment chain recursively while parent borrow guards are
live. This records stable environment-cell pointers without incrementing/decrementing
every `Rc` on every call. The recursion is bounded by
`INLINE_ENVIRONMENT_CHAIN_CAPACITY`, a named ABI constant.

Categorically this changes no execution morphism: environment access receives the same
context object, but ownership is moved to the call boundary and the inner frame receives
a borrowed capability. In Lisp terms the frame derives its pointer view from the single
environment source of truth rather than copying an ownership path.

Acceptance: closure/captured/global/name-cache tests plus the full release suite and
smoke; alternating full-suite A/B against Task 168. Accept only if aggregate performance
improves and every component clears the standing floor.

## Result

Implemented and measured, then rejected and reverted. The candidate stored the executing
environment as a call-scoped pointer, passed `Env` by reference through the user-call
path, and built the bounded environment chain recursively without cloning any parent
`Rc`. All 81 release tests and complete smoke passed.

The ten-pair, 500 ms Richards comparison at
`reports/task169-borrowed-context-richards-ab-10/comparison.txt` measured 757 for Task
168 versus 753 for the candidate, or **-0.53%**. Because the targeted call-heavy suite
was negative, no full-suite comparison was justified. The candidate is preserved at
`/tmp/deegen-task169-borrowed-context`, SHA-256
`4de60b6b83dcbfea3814d21d7d99d4cb20f9eeb125f77396222abd29d1c0574f`,
and its smoke evidence is `reports/task169-borrowed-context-smoke.jsonl`.

The pointer plumbing cost more than the avoided reference-count operations. Task 168's
contiguous frame remains accepted. The next Task 146 step must eliminate the Rust call
transition and nested executor frame together rather than changing ownership of one
context field.
