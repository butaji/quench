# test262 jit — Quench vs Node vs Bun

host: 4 CPUs, Intel(R) Xeon(R) Processor @ 2.10GHz; jobs 4; elapsed 1055s; excluded 825 cases
versions: quench 304a28fcce38d70ccbc653c28e54344f67ab38d0 · node v26.12.1 · bun 1.4.2

| engine | cases | passed | timeouts | CPU s | wall s | RSS median MiB | RSS p95 MiB | RSS max MiB |
|---|---|---|---|---|---|---|---|---|
| quench | 52581 | 52528 | 0 | 763.2 | 857.2 | 12.2 | 13.8 | 1698.4 |
| node | 52581 | 51284 | 0 | 2055.3 | 2264.1 | 45.6 | 47.4 | 1559.6 |
| bun | 52581 | 51732 | 0 | 950.7 | 1035.1 | 21.9 | 27.4 | 2241.1 |

Quench / other (lower is better; a win needs <= 0.80):

| metric | vs node | vs bun |
|---|---|---|
| cpu_s | 0.37 | 0.80 |
| wall_s | 0.38 | 0.83 |
| rss_median_mib | 0.27 | 0.56 |
| rss_max_mib | 1.09 | 0.76 |

Areas where Quench is not 20% better than every engine on every metric: 41 of 261

| area | cases | short of the margin |
|---|---|---|
| annexB/built-ins | 241 | cpu_s vs bun 0.86x; wall_s vs bun 0.85x |
| built-ins/Array | 3081 | rss_max_mib vs node 1.51x; rss_max_mib vs bun 2.02x |
| built-ins/Atomics | 389 | cpu_s vs node 3.67x; cpu_s vs bun 9.29x; wall_s vs node 4.85x; wall_s vs bun 12.04x |
| built-ins/RegExp | 1879 | cpu_s vs node 0.87x; cpu_s vs bun 2.18x; wall_s vs node 0.89x; wall_s vs bun 2.23x; rss_max_mib vs bun 0.87x |
| built-ins/String | 1223 | cpu_s vs bun 1.21x; wall_s vs bun 1.18x |
| built-ins/TypedArray | 1446 | cpu_s vs node 0.85x; cpu_s vs bun 1.78x; wall_s vs node 0.82x; wall_s vs bun 1.78x; rss_max_mib vs node 19.82x; rss_max_mib vs bun 32.61x |
| built-ins/decodeURI | 55 | cpu_s vs bun 3.16x; wall_s vs bun 3.13x |
| built-ins/decodeURIComponent | 56 | cpu_s vs bun 3.03x; wall_s vs bun 2.97x |
| built-ins/encodeURI | 31 | cpu_s vs bun 1.50x; wall_s vs bun 1.45x |
| built-ins/encodeURIComponent | 31 | cpu_s vs bun 1.45x; wall_s vs bun 1.44x |
| built-ins/parseInt | 55 | cpu_s vs bun 0.84x; wall_s vs bun 0.82x |
| harness/assert-notsamevalue-zeros.js | 1 | wall_s vs bun 0.91x |
| harness/assert-true.js | 1 | wall_s vs bun 0.81x |
| harness/asyncHelpers-throwsAsync-func-never-settles.js | 1 | wall_s vs bun 0.83x |
| harness/byteConversionValues.js | 1 | wall_s vs bun 0.85x |
| harness/deepEqual-object.js | 1 | cpu_s vs bun 0.85x; wall_s vs bun 0.90x |
| harness/nativeFunctionMatcher.js | 1 | cpu_s vs bun 0.81x; wall_s vs bun 0.85x |
| harness/propertyhelper-verifynotconfigurable-configurable.js | 1 | wall_s vs bun 0.93x |
| harness/propertyhelper-verifynotenumerable-enumerable-symbol.js | 1 | wall_s vs bun 1.14x |
| harness/propertyhelper-verifynotenumerable-enumerable.js | 1 | cpu_s vs bun 0.92x; wall_s vs bun 1.21x |
| harness/propertyhelper-verifynotwritable-writable.js | 1 | cpu_s vs bun 0.87x |
| harness/propertyhelper-verifywritable-not-writable.js | 1 | wall_s vs bun 0.83x |
| harness/proxytrapshelper-default.js | 1 | wall_s vs bun 0.93x |
| harness/sta.js | 1 | wall_s vs bun 0.85x |
| harness/verifyProperty-noproperty.js | 1 | wall_s vs bun 0.84x |
| harness/verifyProperty-string-prop.js | 1 | wall_s vs bun 0.83x |
| harness/verifyProperty-undefined-desc.js | 1 | wall_s vs bun 1.02x |
| intl402/default-locale-is-supported.js | 1 | cpu_s vs bun 0.80x; wall_s vs bun 1.30x |
| intl402/language-tags-invalid.js | 1 | cpu_s vs bun 0.83x; wall_s vs bun 0.89x |
| intl402/language-tags-with-underscore.js | 1 | cpu_s vs bun 1.22x; wall_s vs bun 2.36x |
| intl402/supportedLocalesOf-empty-and-undefined.js | 1 | cpu_s vs bun 1.02x; wall_s vs bun 1.02x |
| intl402/supportedLocalesOf-locales-arg-coered-to-object.js | 1 | cpu_s vs bun 0.98x; wall_s vs bun 0.90x |
| intl402/supportedLocalesOf-locales-arg-empty-array.js | 1 | cpu_s vs bun 0.93x; wall_s vs bun 0.88x |
| intl402/supportedLocalesOf-test-option-localeMatcher.js | 1 | wall_s vs bun 1.11x |
| intl402/supportedLocalesOf-throws-if-element-not-string-or-object.js | 1 | cpu_s vs bun 0.99x; wall_s vs bun 1.00x |
| language/comments | 51 | cpu_s vs bun 2.65x; wall_s vs bun 2.56x; rss_max_mib vs node 0.95x; rss_max_mib vs bun 1.61x |
| language/expressions | 11033 | rss_max_mib vs bun 0.97x |
| language/identifiers | 268 | cpu_s vs node 3.10x; cpu_s vs bun 7.05x; wall_s vs node 2.90x; wall_s vs bun 6.73x |
| staging/decorators | 3 | cpu_s vs bun 0.83x |
| staging/set-is-subset-table-receiver-cleared.js | 1 | wall_s vs bun 0.86x |
| staging/sm | 1406 | cpu_s vs node 0.82x; cpu_s vs bun 1.16x; wall_s vs node 0.80x; wall_s vs bun 1.18x |
