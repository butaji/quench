# Glossary

## Application gate

A real npm application executed under Node 24 and quench-node with comparable
observable results. Required gates cover web server, CLI, package loader,
streams, crypto, and database/SDK workloads.

## Compatibility manifest

The versioned JSONC file under `tests/node-compat/` that records the expected
status of upstream Node fixtures and their exceptions.

## Platform-limited

Behavior that cannot be provided on a target because it depends on unavailable
operating-system, native, network, or runtime facilities. It is tracked
explicitly rather than counted as an unexplained failure.

## Focused stage

A small readable compatibility regression under `tests/node-compat/stage-N/`.

## Node oracle

The pinned Node 24 minor release used as the behavioral reference for
differential comparisons.

## Broad compatibility

The Node 24 contract across Linux x86_64, Linux ARM64, macOS, and Windows,
excluding native addons and Node-API, with explicit classifications for
unsupported or platform-limited behavior.

## Application gate set

The required workload classes: web server, CLI, package loader, streams,
crypto, and database/SDK.

## Reproducible report

A release artifact recording the runtime version, Node oracle, platform,
manifest revision, exact fixtures and applications, and their results.

## Data-first IR

The normalized representation of API declarations that is the source for
generated registration, wrappers, validation, and routine tests.

## Generated adapter

A reusable wrapper emitted from the data-first IR for a standard Node calling
convention. It is an output, not an additional source of truth.

## Minimum maintainable LOC

The smallest implementation that remains auditable: duplicated mechanics are
generated, while declarations and genuinely special behavior remain readable.
