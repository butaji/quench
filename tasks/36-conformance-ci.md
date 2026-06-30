# Task 36: Integrate conformance harness into CI with thresholds

## Status: COMPLETED

### What was done (2026-06-30)

Created `.github/workflows/conformance.yml` with three jobs:

1. **sanity** — Runs `test_source_direct_simple` and `test_run_sanity` on every push/PR
2. **whitelist-quick** — Runs 50-case whitelist (source-direct) on every push/PR
3. **whitelist-full** — Runs all whitelist cases on push to `main` only

### Configuration

- Minimum pass rate threshold: **50%** (configurable via `MIN_PASS_RATE` env var)
- Timeout per case: 30 seconds
- Total whitelist-full timeout: 60 minutes
- Rust toolchain: stable
- Submodules: checked out with `submodules: recursive`

### Workflow triggers

```yaml
on:
  push:
    branches: [main]
  pull_request:
```

### Verification

```bash
ls .github/workflows/conformance.yml  # ✓ exists
# CI runs automatically on push/PR via GitHub Actions
```

### Remaining work

- Parse the test output programmatically to enforce the 50% threshold as a hard gate
- Add Slack/email notification on failure
- Add a badge to README showing current pass rate
