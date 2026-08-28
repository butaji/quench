# ADR 0005: One-word execution values

Registers, slots, elements, completions, and dispatch boundaries use one
fixed-width word. Immediates are direct; identity-bearing values are typed,
aligned heap references. Tags/accessors come from one declaration; invalid or
noncanonical words are rejected.

Movement copies a word; ownership changes only at explicit boundaries. Decode
to broad semantic `Value` only where an operation/API requires it. Fast and
slow paths share words and ownership rules, with complete fallback. Tests prove
lossless encoding, ownership, observables, and invalid-word behavior.
