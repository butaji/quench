# JavaScript microbenchmarks

The checked-in programs form a deterministic stress corpus for measuring the
ordinary `quench-node` path against Node. The manifest is the single source of
truth for workload names and units.

Rules:

- Keep setup outside the timed operation and keep results observable.
- Validate corpus and manifest consistency before measuring.
- Keep timing, memory, profiling, and tracing evidence in separate runs.
- Report raw repeated samples and compare only matching corpus and build
  revisions.
- Treat benchmark output as measurement evidence, never as compatibility
  proof or a runtime dispatch signal.
