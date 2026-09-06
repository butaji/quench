# ADR: Keep generated outputs and stale wiring out of the repository

## Decision

Remove tracked Cargo output trees and generated benchmark `dist/` files. Ignore
all root `target-*` directories and purge the generated blobs from published
history. Remove the obsolete root `quench-runtime` gitlink, remove the unused
TypeScript submodule declaration, and register the live benchmark submodule.

Move the historical benchmark ledger and budget into `docs/evidence/`, where
their provenance is explicit and they cannot be mistaken for active evidence.

## Rationale

Cargo fingerprints, libraries, assembly, and binaries are reproducible build
outputs, not source or release artifacts. The root runtime gitlink is empty and
the active implementation is `crates/quench-runtime`. The benchmark `dist/`
files are ignored generated concatenations; current runners consume the pinned
`v8-v7` fixtures directly. The archived ledger remains useful for historical
context but contains an absolute, non-reproducible source path.

## Intentionally retained

The Node, Test262, and Wasm test submodules; their fixture packages; the
micros/deegen corpora; and the separate Node/Test262 stage specifications remain
because active runners or documentation reference them.
