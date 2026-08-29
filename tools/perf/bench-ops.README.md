# Per-operation measurement

Use bounded deterministic probes of reusable VM operations, not named
fixtures. Report time, memory, allocation proxies, and missing/timed-out data
explicitly. Probes preserve the ordinary VM's observable behavior and inform
engineering; they never choose runtime semantics.
