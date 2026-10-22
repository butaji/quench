# Filesystem URL, callback and URI parity checkpoint

Repair source `7c628a92c`, integrated source `7e47ca977`.
Oracle is official Node v26.12.0. The frozen integrated executable passes all
263 serial API comparisons, 622 runtime tests, workspace/all-target checks,
framework-core 43/43, and all nine no-JIT web framework correctness combinations.
No throughput measurement or benchmark win is claimed.

Rust now observes URL-like getters and options in Node order, preserves getter
exceptions, uses original built-ins, validates URL scheme/host/encoded separators,
and retains lone UTF-16 surrogates. Invalid path errors retain their original URL
input with Node descriptors and property order. stat/lstat/statfs callback exceptions
escape once instead of being caught as operation failures and delivered again.
The shared URI codec reports Node's canonical URIError message for malformed inputs.

Focused comparisons cover values, thrown identity, getter order, mutated globals,
Windows/POSIX path rules, malformed escapes, public error metadata, and both success
and operation-error callback completion. Initial failed comparisons are retained:
the URI codec message differed first; the next comparison found error field order.
Both are repaired in the qualified snapshot.

Instruction counts use the exact frozen executable and identical original baseline
binary/input paths. Raw counts and hashes are retained. Counts include shared-lane
changes, are from the iteration profile, and do not isolate repair costs or measure
production throughput or RSS. Older TOD-135 findings remain tied to their own baselines.

`url-getter-probe` retains before-repair Node/Quench discrepancies. `url-error-probe`
records a subsequent open discrepancy in coded error formatting/order. Later byte URL
and coded error repairs are not qualified by these snapshot results. The current byte
API incorrectly rejects some encoded separators; its complete semantics remain work
in progress. Broader Node parity and the web throughput goal remain open.
