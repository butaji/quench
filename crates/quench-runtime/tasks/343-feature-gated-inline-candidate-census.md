# 343 — Feature-gated initial inline-candidate census

Status: complete

Define the first Task 20 eligibility decision as data in `src/inline_plan.rs`: exact
arity, noncapturing frame, no dynamic `arguments`, no nested call, straight-line control,
and named code/frame budgets. Every candidate has exactly one canonical result from the
`InitialInlineDecision` sum. Two tests cover rejection precedence and budget boundaries.

The census is compiled only with Cargo feature `inline-census`. The default release build
contains neither its counters nor its call-edge branch. An initial always-present version
screened at -1.08%; it was rejected and replaced by the feature boundary before acceptance.

One 300 ms instrumented V8v7 pass found:

| Suite | Cached calls | Initially eligible | Main later class |
|---|---:|---:|---|
| Richards | 5,327,598 | 15.81% | nested call 49.15%; control 34.98% |
| DeltaBlue | 7,686,975 | 37.88% | nested call 35.98%; control 25.51% |
| Crypto | 1,412,823 | 0.20% | control 72.03%; nested call 27.53% |
| RayTrace | 2,281,082 | 14.02% | `arguments` 46.71%; nested call 37.81% |
| Earley-Boyer | 8,690,130 | 4.87% | nested call 46.31%; arity 35.61% |
| RegExp | 34,575 | 95.33% | nested call 4.65% |
| Splay | 2,475,595 | 10.54% | nested call 83.63% |
| Navier-Stokes | 2,423 | 15.97% | control 61.21%; nested call 19.19% |

The initial subset has material reach in Richards, DeltaBlue, RayTrace, RegExp, and Splay.
The next implementation is the quote-level alpha-renaming and return-continuation rewrite;
the census must not be mistaken for that implementation.

Instrumented build command: `cargo build --release --features inline-census`.
Instrumented binary SHA-256:
`9f1fc8cd1e47e40296b5226bbcbfbb478f0e809b4ed99d4edede0c7b7e3da565`.

The actual default workspace binary contains no census edge. Its final five-pair 500 ms
compound comparison at `reports/task343-current-compound-full-ab-5-long/comparison.txt`
measures Task 336 at 2146.19 and the current binary at **2203.74 (+2.68%)**. Richards
improves 6.47%, DeltaBlue 5.22%, RayTrace 4.42%, and Earley-Boyer 3.06%; the worst suite
is RegExp at -0.14%. Default binary SHA-256:
`7ad3063c7406e7a81cffad4a5b49a3c4e7dbda9ed1b42a5abbf921fcaf4f8d35`.
