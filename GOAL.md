# Goal: stages 95–103

Bring test262 stages 95–103 to 100% passing through the canonical runner:
Intl Array, BigInt, Collator, Date, DateTimeFormat, DisplayNames,
DurationFormat, FallbackSymbol, and Intl. Preserve ECMA-402 option-getter
order, locale canonicalization, prototype/accessor descriptors, formatting,
completion behavior, and errors. Use the shared semantic path; do not alter
the harness or test262. Re-run owned stages plus earlier regressions, run
quality checks, and commit verified changes.
