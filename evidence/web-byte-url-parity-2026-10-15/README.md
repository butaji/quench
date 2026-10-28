# Byte file URL and coded error qualification

Repair source `dd7d7c4f4`. Oracle: official Node v26.12.0.

The frozen executable passes all 264 serial API comparisons, workspace/all-target
checks, framework-core43/43 and all nine no-JIT web correctness combinations.
Runtime qualification is reused from the 622-test result at `7e47ca977`: all runtime
and dependency sources are unchanged; the intervening repair changes only Node URL,
Node coded-error policy and the differential test. No throughput win is claimed.

Both URL APIs share native URL-like validation, options/getter ordering and scheme/
host rules. The string API rejects encoded separators and malformed UTF-8; the byte
API correctly permits encoded separators and retains raw invalid UTF-8 and malformed
percent escapes. Rust calls the configured, unmodified Node `internal/data_url`
percentDecode function as guest policy inside Quench. No external runtime executes
application behavior. The canonical Buffer export and its observable conversion
hooks are preserved even if the global Buffer binding is replaced. Windows drive
and UNC handling retain byte semantics. Coded error prototypes and the distinct
message/code/constructor-field orders use one native Node error owner.

Focused comparisons include raw bytes, both platforms' path rules, URL-like getters,
thrown identity, original error input, descriptor/key ordering, mutated globals and
public Buffer.from hooks. The full API run also covers earlier clone, filesystem,
HTTP, stream, TLS, crypto and event behavior within its curated scope.

All eight iteration instruction cases are recorded with original baseline reuse and
identical input paths. Raw counts and binary hashes identify the exact snapshot.
Shared-lane changes are included; these are not isolated repair attribution,
production throughput or RSS. Regressions remain findings in TOD-135.

`fs-byte-path-probe` records the next confirmed open repair: filesystem metadata APIs
must accept Uint8Array, avoid Buffer.toString hooks and reject NUL paths with Node's
validation error. Those findings are not repaired in this snapshot. Total Node parity,
shared host-object clone protocols, workers and broad APIs remain open; optimization
and a benchmark win remain gated on the user's parity requirement.
