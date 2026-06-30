# Task 44: Document the conformance harness architecture and workflow

## Status: COMPLETED

### What was done

Created `docs/conformance.md` with documentation covering:

- **Architecture overview** — test corpus structure, key components (`TestCase`, `TestResult`, `RunMode`, `ConformanceReport`)
- **Three run modes** — BaselineJs, SourceTs, Hybrid with use cases
- **Skip rules** — complete list of skip conditions
- **Multi-file cases** — how `// @filename:` markers work
- **Configuration-specific baselines** — directive handling
- **Emit helpers** — the `EMIT_HELPERS` constant
- **Running tests** — quick sanity, 50-case, full whitelist, all tests
- **Interpreting results** — understanding pass/fail/skip categories
- **Updating the whitelist** — how to add/remove categories
- **Local pass-rate gate** — `MIN_PASS_RATE` environment variable; no external CI
- **Common issues** — ReferenceError, parse errors, missing baselines, timeouts
- **Architecture diagram** — ASCII flow chart from `.ts` source to JSON report

### Files

- `docs/conformance.md`
- `README.md` links to it

### Verification

```bash
ls docs/conformance.md
```
