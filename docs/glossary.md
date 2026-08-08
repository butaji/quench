# Glossary

## Application gate

A real npm application executed under Node 24 and quench-node with comparable
observable results. The first gates are Hono and a small CLI tool.

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
