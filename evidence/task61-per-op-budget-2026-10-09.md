# Task 61 per-op instruction budget — 2026-10-09

Three CJS-shaped repetitions per engine and case; active process counters subtract the corresponding zero-iteration process. The `Quench / best ref` column uses the lowest instruction count among successfully measured rqj, qjs, Node `--jitless`, and Bun with `BUN_JSC_useJIT=0`. This is a ranking diagnostic, not fixture performance evidence.

| Case | Quench | rqj | qjs | Node jitless | Bun no-JIT | Best reference | Quench / best | Quench cycles/iteration |
|---|---:|---:|---:|---:|---:|---|---:|---:|
| array-literal | 5288.0 | 1644.3 | 792.3 | 629.2 | 581.8 | bun_no_jit | 9.1× | 775.4 |
| array-read | 1731.5 | 414.1 | 182.0 | 269.2 | 162.1 | bun_no_jit | 10.7× | 216.3 |
| array-write | 2452.2 | 420.1 | 182.0 | 298.7 | 183.1 | qjs | 13.5× | 310.3 |
| char-code-at | 2671.0 | 615.0 | 556.1 | 429.9 | 326.1 | bun_no_jit | 8.2× | 339.0 |
| closure-create | 24758.8 | 1143.5 | 2404.7 | 362.7 | 407.1 | node_jitless | 68.3× | 3827.9 |
| compound-index | 3077.2 | — | 227.0 | 411.8 | 318.1 | qjs | 13.6× | 382.4 |
| construct | 9411.3 | 1446.1 | 1452.4 | 574.2 | 589.5 | node_jitless | 16.4× | 1458.2 |
| field-read | 1289.1 | 344.0 | 153.0 | 194.6 | 122.0 | bun_no_jit | 10.6× | 145.3 |
| from-char-code | 13778.7 | 1750.8 | 659.1 | 431.1 | 389.6 | bun_no_jit | 35.4× | 3457.0 |
| instanceof | 4632.3 | — | 586.1 | 413.2 | 364.0 | bun_no_jit | 12.7× | 615.9 |
| local-call | 2977.5 | 822.2 | 396.1 | 364.1 | 303.1 | bun_no_jit | 9.8× | 401.8 |
| local-store | 1006.2 | 260.0 | 110.0 | 145.8 | 89.1 | bun_no_jit | 11.3× | 114.7 |
| math-floor | 9757.4 | 622.7 | 440.0 | 445.4 | 523.6 | qjs | 22.2× | 2522.1 |
| object-literal | 19528.1 | 1595.9 | 1888.3 | 373.5 | 499.0 | node_jitless | 52.3× | 3180.2 |
| regexp-exec | 22115.3 | — | 2489.2 | 980.4 | 903.6 | bun_no_jit | 24.5× | 3559.0 |
| regexp-replace | 46081.1 | — | 4972.8 | 1751.5 | 1910.4 | node_jitless | 26.3× | 9152.1 |
| substring | 7072.2 | 1258.2 | 838.3 | 552.9 | 422.7 | bun_no_jit | 16.7× | 1024.2 |
| typeof | 5068.3 | 910.7 | 208.0 | 190.0 | 214.0 | node_jitless | 26.7× | 741.6 |

RQJ could not execute the compound-index case (`value is not callable`), the `instanceof` case (`unsupported binary operator 21`), or either RegExp case (outside its supported subset); these are marked unavailable rather than treated as performance results. All other measured engines completed each active and zero-iteration script successfully.
