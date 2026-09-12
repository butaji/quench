# 06 — Word-sized tagged Value

Status: complete

Replace the fat Rust enum with one 64-bit tagged word: unboxed doubles and tagged immediates/pointers. This is the shared value representation for registers, frames, object slots, helper boundaries, and stencil ABI.

Current state: `RawValue` tags and a transparent `Value` wrapper exist; every value is
one 64-bit word. Numbers are unboxed doubles and immediate/pointer variants use tagged
NaN payloads. Heap variants temporarily retain `Rc` ownership through manual
clone/drop dispatch. The current 61-test release suite includes one-word layout and
clone/drop coverage for every heap tag.

Measurement: `reports/value-word-ab/comparison.txt` records an aggregate 428.023 → 428.004, effectively neutral. The full smoke completed seven suites; Earley-Boyer exceeded the 45-second timeout. This is a representation prerequisite, not an accepted speed win.

Acceptance is complete: `size_of::<Value>() == 8`, clone/drop tests for every heap tag,
full V8v7 smoke, and alternating A/B evidence are all present. The neutral isolated
measurement is expected for a representation prerequisite. Follow-up [[09-object-memory-model]]
owns removal of heap refcount/borrow costs; that remaining work does not make `Value`
a Rust enum or reopen this task.
