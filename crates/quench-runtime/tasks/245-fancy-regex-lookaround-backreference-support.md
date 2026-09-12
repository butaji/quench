# 245 — Adopt `fancy-regex` for lookaround/backreference support instead of a custom engine

Status: planned

Regex is exactly the kind of problem this project should not re-solve from scratch:
matching-with-backreferences/lookaround under a hybrid fast-automaton/backtracking-
fallback strategy is a well-trodden, already-solved problem (V8's irregexp does this
internally; PCRE, Oniguruma, and .NET's regex engine all do some form of it), and the
Rust ecosystem already has a maintained, drop-in implementation of exactly that
strategy. [[87-regexp-stencil-compilation]] already correctly concluded a hand-written
matcher stencil is not the next regex priority; this task closes the actual remaining
gap — not performance, but **coverage/correctness** — using an existing solution rather
than building one.

**The gap is real and already in the benchmarked corpus, not hypothetical.** Rust's
`regex` crate (used today, per `src/main.rs:18` and `Cargo.toml`) deliberately does not
support lookaround or backreferences — a documented design choice, made specifically to
guarantee linear-time matching. Grepping the actual corpus finds both features in
actual use:
- `/private/tmp/js-engine-benchmark/v8-v7/regexp.js:1098`: `/HF(?=;)/i.exec(str56);` —
  a lookahead assertion.
- `/private/tmp/js-engine-benchmark/v8-v7/regexp.js:1303`:
  `var re94 = /^(\[) *@?([\w-]+) *([!*$^~=]*) *('?"?)(.*?)\4 *\]/;` — a backreference
  to the fourth capture group (a real jQuery/Sizzle-derived CSS-selector-parsing regex,
  a common real-world pattern shape).

Per [[142-shared-regexp-literal-kernels]]'s own accepted implementation, a pattern the
host matcher rejects is stored as an immutable error and raised only when that opcode
executes — meaning, as implemented today, these two patterns do not silently
mismatch, they **throw at runtime**, which is either a live correctness bug in the
regexp.js suite right now or (if the suite's harness tolerates the resulting exception
without failing) a silent coverage gap depressing whatever the RegExp suite component
actually measures. Either way this needs to be confirmed and fixed, not assumed benign.

**The fix:** replace (or wrap) the `regex` crate with `fancy-regex`, which is
purpose-built for exactly this situation — it delegates every sub-expression it can to
the underlying `regex` crate's linear-time automaton (so the common case, patterns
without lookaround/backreferences, keeps [[142]]'s existing performance characteristics
unchanged) and falls back to a backtracking VM only for the specific sub-expressions
that actually need `(?=`/`(?!`/`(?<=`/`(?<!`/backreferences — the same hybrid
architecture V8's irregexp itself uses internally, now available as a maintained
dependency instead of something this project would otherwise have to build and validate
from scratch.

Concrete steps:
1. Confirm the current failure mode for both corpus patterns above with a direct test
   (does `HF(?=;)` currently throw, silently mismatch, or something else) — do not
   assume; this project's own discipline throughout this thread has been to verify
   corpus behavior directly before proposing a fix.
2. Add `fancy-regex` as a dependency and route regex compilation ([[142]]'s
   `RegExpLiteralKernel::Compiled`) through it; confirm patterns without fancy features
   compile to the same fast path as today (verify by checking `fancy-regex`'s own
   delegation actually reaches the underlying `regex` crate for a plain pattern, not
   merely assuming it from the crate's documentation).
3. Add correctness tests for both corpus patterns (and the general lookaround/
   backreference feature surface, since real-world JS source uses these constructs
   commonly outside this specific corpus too) confirming correct match results.

Acceptance: `/HF(?=;)/i` and `re94`'s backreference pattern both match correctly against
representative input, verified by a direct test; non-fancy patterns show no performance
regression relative to [[142]]'s existing accepted baseline, verified by alternating
A/B on the RegExp suite; the full V8v7 suite shows no regression; a stated coverage note
records whether the corpus's actual measured RegExp score changes as a result of this
fix (a previously-throwing pattern now correctly matching could move the score in either
direction depending on how the benchmark harness scored the failure before).

Primary sources:
- `fancy-regex` (the adopted solution): <https://github.com/fancy-regex/fancy-regex>
- `regex` crate's own documented lack of backreference/lookaround support (the reason
  this gap exists): standard `regex` crate documentation, referenced via
  [[142]]'s existing use of the crate.

Source for corpus evidence: `/private/tmp/js-engine-benchmark/v8-v7/regexp.js:1098,1303`
(local V8v7 corpus checkout).
