# 227 — O(1) indexed character access for String.charAt/charCodeAt

Status: planned

The highest-value remaining O(n)→O(1) gap on a hot *load* path, found by auditing every
`native_string_*`/`native_array_*` function in `src/main.rs` for non-constant-time
per-call cost and cross-checking against the real V8v7 corpus.

`native_string_char_code_at` and `native_string_char_at` (`src/main.rs:4853-4870`) are
both implemented as `string_this(this).chars().nth(i)` — `Iterator::nth` on a UTF-8
`chars()` iterator walks the string from byte offset 0 every single call, so each
`charAt`/`charCodeAt` call is O(i), not O(1). JS strings are specified as UTF-16
code-unit-indexed with O(1) random access in every production engine; this
implementation is silently O(n) per access instead.

**Corpus evidence, not a hypothetical:** grepping
`/private/tmp/js-engine-benchmark/v8-v7/*.js` finds `charAt`/`charCodeAt` at 8 call
sites in `crypto.js` and 25 in `earley-boyer.js`, and at least two of them are exactly
the pathological shape — a monotonic index walking one character at a time across a
whole string:
- `earley-boyer.js:2156`: `return this.str.charAt(this.pos++);` — the Scheme reader's
  character-at-a-time lexer (`// we are going to do some charAts on the str.` at line
  2146 is the source's own comment acknowledging the access pattern). A lexer that reads
  an n-character source one `charAt` at a time is, with this implementation, O(n²) in
  the source length instead of the O(n) every real engine gives it.
- `crypto.js:1484`: `while(i >= 0 && n > 0) ba[--n] = s.charCodeAt(i--);` — a
  decreasing-index scan converting a string to a byte array, same O(n²) pathology.

`native_string_substr`'s `s.chars().skip(start).take(len).collect()`
(`src/main.rs:4871-4881`) has the same underlying issue: `skip(start)` walks `start`
characters from the beginning every call.

**Fix, and how it relates to existing tasks.** This is squarely [[11-string-representation]]'s
and [[190-multi-representation-utf16-javascript-strings]]'s scope (both still
`planned`), but neither task's text calls out *this specific, corpus-verified O(n)→O(n²)
regression* as motivating evidence, so it is easy for either to land a representation
change that fixes allocation/interning concerns while leaving indexed access
accidentally still O(n) (e.g. keeping a `String`/`Vec<char>` internally and only
optimizing the interning path). Make O(1) indexed character access an explicit,
separately-tested acceptance criterion of whichever of [[11]]/[[190]] lands the
representation change: a flat, randomly-indexable backing (a `Vec<u16>`/`Box<[u16]>` for
the general case, or a `Vec<u8>`/`Box<[u8]>` Latin1 fast path when every character fits
in one byte, mirroring the two-width design [[190]] already cites from V8's actual
string implementation) gives `charAt`/`charCodeAt`/`substr`-style indexed access true
O(1)/O(length-of-slice) cost with no per-call linear walk from the start.

Acceptance: `charAt`/`charCodeAt` on a string of length n cost O(1), verified by a
benchmark reading a large string end-to-end one character at a time via increasing index
showing linear (not quadratic) total time as string size grows; `substr`'s cost is
O(result length), not O(start + result length); existing string correctness tests
(including non-ASCII/surrogate-pair edge cases, since a naive UTF-16-code-unit
representation must still get `charCodeAt` semantics right for astral characters) pass
unchanged; alternating A/B on crypto and earley-boyer specifically (the two suites with
confirmed corpus usage) shows no regression and documents any measured gain.

## 2026-09-10 experiments: two rejected local caches

Two general-purpose implementation slices were built, correctness-tested, and measured
against the same saved pre-change binary (`ac36b11c...`). Neither passed the aggregate
performance gate, so both were removed from production code and this task remains
planned:

1. A universal `HeapString { text, code_unit_len }` representation made ASCII access
   direct and cached a UTF-16 view for non-ASCII strings, but enlarged every string
   allocation even when the program never indexed it. Five alternating focused runs in
   `reports/task227-indexed-string-focused-ab-5/comparison.txt` measured Crypto
   1347→1339 (-0.59%), EarleyBoyer 2257→2173 (-3.72%), aggregate 1743.61→1705.77
   (-2.17%).
2. A bounded four-entry VM-local cache preserved the existing `Rc<String>` layout and
   indexed only strings which reached `charAt`, `charCodeAt`, or `length`. All 91 release
   tests passed. Five alternating focused runs in
   `reports/task227-index-cache-focused-ab-5/comparison.txt` measured Crypto 1357→1367
   (+0.74%), EarleyBoyer 2283→2237 (-2.01%), aggregate 1760.12→1748.71 (-0.65%).

The result falsifies the idea that a side cache is enough: its lookup and mutable-borrow
cost remains on every indexed operation, while these call sites still pay generic method
lookup and native-call overhead. The next implementation should be the actual Task 190
two-width string representation, paired with a direct string-index/method stencil, rather
than another cache layered beside `Rc<String>`.

Sources: the local V8v7 corpus at
`/Users/admin/Code/GitHub/quench/quench-bench/js-engine-benchmark/v8-v7/earley-boyer.js:2146-2156`
and `crypto.js:1484`; V8's directly indexed sequential one-byte/two-byte representations
and lazy cons/sliced forms: <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/docs/objects/strings.md>.
