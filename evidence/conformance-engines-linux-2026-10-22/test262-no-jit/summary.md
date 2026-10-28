# test262 no-jit — Quench vs Node vs Bun

host: 4 CPUs, Intel(R) Xeon(R) Processor @ 2.10GHz; jobs 4; elapsed 1072s; excluded 825 cases
versions: quench 304a28fcce38d70ccbc653c28e54344f67ab38d0 · node v26.12.1 · bun 1.4.2

| engine | cases | passed | timeouts | CPU s | wall s | RSS median MiB | RSS p95 MiB | RSS max MiB |
|---|---|---|---|---|---|---|---|---|
| quench | 52581 | 52528 | 0 | 768.6 | 846.1 | 12.1 | 13.7 | 1696.2 |
| node | 52581 | 51284 | 0 | 2120.2 | 2343.4 | 41.4 | 43.3 | 1556.1 |
| bun | 52581 | 51738 | 0 | 960.9 | 1033.2 | 21.2 | 25.6 | 4246.7 |

Quench / other (lower is better; a win needs <= 0.80):

| metric | vs node | vs bun |
|---|---|---|
| cpu_s | 0.36 | 0.80 |
| wall_s | 0.36 | 0.82 |
| rss_median_mib | 0.29 | 0.57 |
| rss_max_mib | 1.09 | 0.40 |

Areas where Quench is not 20% better than every engine on every metric: 52 of 261

| area | cases | short of the margin |
|---|---|---|
| annexB/built-ins | 241 | cpu_s vs bun 0.83x; wall_s vs bun 0.82x |
| built-ins/Array | 3081 | rss_max_mib vs node 1.62x; rss_max_mib vs bun 2.56x |
| built-ins/ArrayBuffer | 221 | cpu_s vs bun 0.84x; wall_s vs bun 0.86x |
| built-ins/Atomics | 389 | cpu_s vs node 3.70x; cpu_s vs bun 10.24x; wall_s vs node 4.74x; wall_s vs bun 13.28x; rss_max_mib vs bun 0.87x |
| built-ins/RegExp | 1879 | cpu_s vs bun 1.28x; wall_s vs bun 1.21x; rss_max_mib vs bun 0.95x |
| built-ins/String | 1223 | cpu_s vs bun 1.33x; wall_s vs bun 1.30x |
| built-ins/TypedArray | 1446 | cpu_s vs node 0.96x; cpu_s vs bun 2.22x; wall_s vs node 0.90x; wall_s vs bun 2.15x; rss_max_mib vs node 22.31x; rss_max_mib vs bun 43.61x |
| built-ins/TypedArrayConstructors | 738 | rss_max_mib vs bun 0.89x |
| built-ins/decodeURI | 55 | cpu_s vs bun 2.26x; wall_s vs bun 2.19x |
| built-ins/decodeURIComponent | 56 | cpu_s vs bun 2.22x; wall_s vs bun 2.15x |
| built-ins/encodeURI | 31 | cpu_s vs bun 1.48x; wall_s vs bun 1.41x |
| built-ins/encodeURIComponent | 31 | cpu_s vs bun 1.24x; wall_s vs bun 1.22x |
| built-ins/parseFloat | 54 | cpu_s vs bun 0.82x; wall_s vs bun 0.81x |
| built-ins/parseInt | 55 | cpu_s vs bun 0.90x; wall_s vs bun 0.90x |
| harness/assert-samevalue-same.js | 1 | wall_s vs bun 0.85x |
| harness/assert-samevalue-tostring.js | 1 | wall_s vs bun 1.18x |
| harness/assert-throws-same-realm.js | 1 | wall_s vs bun 0.94x |
| harness/assert-tostring.js | 1 | cpu_s vs bun 0.82x; wall_s vs bun 0.83x |
| harness/assert-true.js | 1 | wall_s vs bun 1.05x |
| harness/asyncHelpers-throwsAsync-func-never-settles.js | 1 | wall_s vs bun 0.84x |
| harness/compare-array-falsy-arguments.js | 1 | wall_s vs bun 0.97x |
| harness/compare-array-same-elements-different-order.js | 1 | wall_s vs bun 0.93x |
| harness/deepEqual-deep.js | 1 | wall_s vs bun 0.95x |
| harness/deepEqual-mapset.js | 1 | cpu_s vs bun 0.96x; wall_s vs bun 1.33x |
| harness/deepEqual-object.js | 1 | cpu_s vs bun 0.90x; wall_s vs bun 1.29x |
| harness/deepEqual-primitives-bigint.js | 1 | cpu_s vs bun 0.81x; wall_s vs bun 0.96x |
| harness/detachArrayBuffer-host-detachArrayBuffer.js | 1 | cpu_s vs bun 0.83x; wall_s vs bun 0.83x |
| harness/nativeFunctionMatcher.js | 1 | cpu_s vs bun 0.85x; wall_s vs bun 0.98x |
| harness/propertyhelper-verifyenumerable-enumerable-symbol.js | 1 | cpu_s vs bun 0.81x; wall_s vs bun 1.24x |
| harness/propertyhelper-verifyenumerable-not-enumerable-symbol.js | 1 | wall_s vs bun 0.83x |
| harness/propertyhelper-verifynotenumerable-enumerable-symbol.js | 1 | wall_s vs bun 1.07x |
| harness/propertyhelper-verifywritable-array-length.js | 1 | wall_s vs bun 0.95x |
| harness/verifyProperty-desc-is-not-object.js | 1 | cpu_s vs bun 0.82x |
| harness/verifyProperty-noproperty.js | 1 | wall_s vs bun 0.84x |
| harness/verifyProperty-symbol-prop.js | 1 | cpu_s vs bun 0.80x |
| harness/verifyProperty-value.js | 1 | cpu_s vs bun 1.15x; wall_s vs bun 1.14x |
| harness/wellKnownIntrinsicObjects.js | 1 | cpu_s vs bun 0.95x; wall_s vs bun 0.94x |
| intl402/constructors-taint-Object-prototype-2.js | 1 | wall_s vs bun 0.84x |
| intl402/language-tags-invalid.js | 1 | cpu_s vs bun 1.17x; wall_s vs bun 1.18x |
| intl402/language-tags-with-underscore.js | 1 | cpu_s vs bun 1.22x; wall_s vs bun 1.21x |
| intl402/supportedLocalesOf-empty-and-undefined.js | 1 | cpu_s vs bun 0.99x; wall_s vs bun 1.07x |
| intl402/supportedLocalesOf-locales-arg-coered-to-object.js | 1 | cpu_s vs bun 0.99x; wall_s vs bun 0.96x |
| intl402/supportedLocalesOf-locales-arg-empty-array.js | 1 | cpu_s vs bun 1.49x; wall_s vs bun 1.30x |
| intl402/supportedLocalesOf-taint-Array.js | 1 | wall_s vs bun 0.87x |
| intl402/supportedLocalesOf-test-option-localeMatcher.js | 1 | wall_s vs bun 0.85x |
| intl402/supportedLocalesOf-throws-if-element-not-string-or-object.js | 1 | cpu_s vs bun 1.10x; wall_s vs bun 1.08x |
| language/comments | 51 | cpu_s vs node 0.88x; cpu_s vs bun 2.57x; wall_s vs node 0.88x; wall_s vs bun 2.44x; rss_max_mib vs node 0.88x; rss_max_mib vs bun 3.18x |
| language/expressions | 11033 | rss_max_mib vs bun 1.16x |
| language/identifiers | 268 | cpu_s vs node 3.27x; cpu_s vs bun 7.55x; wall_s vs node 2.95x; wall_s vs bun 7.24x |
| language/module-code | 3 | wall_s vs bun 0.82x |
| staging/decorators | 3 | cpu_s vs bun 0.92x; wall_s vs bun 1.05x |
| staging/sm | 1406 | cpu_s vs bun 0.93x; wall_s vs bun 0.93x |
