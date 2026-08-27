# ADR 0005: One-word execute values

## Principles

- Use one canonical, fixed-width execute word for registers, slots, elements,
  completions, and dispatch boundaries.
- Encode immediate primitives directly and encode identity-bearing values as
  typed, aligned heap references. Keep semantic payloads in their existing
  stores.
- Define tag numbers and accessors from one declaration. Reject unknown tags,
  noncanonical payloads, unaligned references, and unrepresentable pointers.
- Copying a word is the only register move. Retain or release a heap reference
  exactly at ownership boundaries; do not clone semantic values for movement.
- Decode to the wider semantic value only at an API or operation boundary that
  requires it.
- Slow and fast operations consume the same words and ownership rules. A guard
  miss returns to complete ordinary semantics.
- Never maintain a shadow register representation or a second heap identity
  scheme.

## Required evidence

Tests must prove lossless encoding, exact ownership, preserved JavaScript
observables, and correct behavior for invalid tags and references. Layout or
size measurements are diagnostics, not semantic gates.
