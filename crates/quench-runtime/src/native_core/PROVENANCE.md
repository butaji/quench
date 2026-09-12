# Native execution core

This directory is the direct Quench VM hot-path representation. `value_word.rs`
is the one authoritative machine-word layout for registers, slots and native
entry operands; semantic `Value` decoding happens only at explicit boundaries.

The module was extracted from the native execution import and checked into the
runtime so builds are self-contained. The discarded import files were unused
standalone adapters and stencil experiments; keeping them would create a
second representation and a second source of opcode truth.
