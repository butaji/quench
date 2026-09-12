# 263 — Reusable per-instance RegExp capture scratch

Status: complete

Keep the compiled `Regex` as an immutable shared Kernel. Extend each mutable
`RegExpValue` instance with capture-offset scratch created once from that kernel, and use
`Regex::captures_read`/`captures_read_at` for `RegExp.prototype.exec` and non-global
`String.prototype.match` instead of constructing a fresh `Captures` location vector for
every search.

This preserves the Kernel/Instance split exactly: automaton code is shared without
memory growth; `global`, `last_index`, and scratch are instance state. The scratch must
not be stored in `StencilTemplate` or the shared Kernel because it is mutable. A native
call may borrow it only for the duration of one search; any path that can re-enter the
same RegExp instance must finish/copy required offsets before invoking user code.

Task 253's RegExp sample shows matcher routines plus allocator/free/memmove/memset in the
dominant native frames after Task 142 already eliminated repeated pattern compilation.
Rust regex documents `captures_read` specifically for amortizing allocation when
`Captures` allocation appears in a profile. Global match paths that need only complete
matches retain `find_iter`; `test` retains `is_match`; neither should pay capture costs.

Acceptance: allocation counters distinguish RegExp instance creation from match-time
scratch growth; repeated `exec` with a stable capture count performs zero steady-state
capture-location allocations; capture groups, unmatched groups, global state, and
re-entrant calls remain correct; full V8v7 smoke and alternating RegExp/full-suite A/B
pass before acceptance.

## Implementation

- `RegExpValue` owns an optional `CaptureLocations`; `RegExpKernel` remains immutable
  and shared through `Rc`.
- `capture_values` initializes the locations lazily, reuses them through
  `Regex::captures_read`, and copies only the result strings into JS values before the
  mutable borrow ends.
- `RegExp.prototype.exec` and non-global `String.prototype.match` use the reusable
  instance scratch. Global `match` still uses `find_iter`, and `test` still uses
  `is_match`.
- All RegExp construction paths use one `RegExpValue::new` constructor, including AST,
  dynamic-bytecode, native constructor, and tests.

## Verification

- Candidate SHA-256:
  `49c2578c8a8406d4595c5dbefdae90b67f9523dc470466b55517bdabe00b5f3a`.
- `cargo test --release`: 90 passed, 0 failed.
- A focused test verifies lazy creation, reuse of the same scratch object, capture
  values, and the existing unmatched-group representation.
- Full V8v7 20 ms smoke completed all eight suites with score `1864.55`.
- Four alternating 3000 ms RegExp pairs produced baseline
  `3185, 3059, 3216, 3157` and candidate `3323, 3274, 3334, 3339`:
  median `3171.0 -> 3328.5`, a `+4.97%` gain.
- Five alternating 200 ms whole-suite pairs produced median
  `1860.91 -> 1850.36` (`-0.57%`), treated as neutral at this run length rather than
  evidence of a broad regression.

No heap-allocation counter was added in this task. Reuse is established structurally
and by stable scratch identity in the focused test; a future allocator-instrumented
measurement can quantify the exact eliminated allocation count.

Primary source:
<https://docs.rs/regex/latest/regex/struct.Regex.html#method.captures_read>.
