# 284 — Compositional property block tiler

Status: complete

Lift Task 283's own-property transfer from an exact four-bytecode block to a reusable
three-bytecode supernode inside arbitrary basic blocks:

`SemanticRange* ; OwnPropertyLoadStore ; SemanticRange*`

The planner greedily partitions one normalized basic block into a flat sequence of
canonical semantic ranges and independently cooked property supernodes. Composition is
the existing free stencil monoid; every part remains `Connector -> Connector`. A range
kernel stops at its derived `run_end` and transfers to the next labeled part. A property
guard miss enters the canonical block slow adapter at that bytecode and executes the
remaining semantics once.

The property atom is selected only for exact
`LoadLocal(receiver); GetStatic(key); StoreLocal(destination)` flow with globally dead
temporary registers. The selector uses no property spelling, source location, benchmark
identity, or execution count. Exact whole-block stencils retain precedence.

Acceptance: partition/label/range tests, direct execution tests, release suite, selection
counts showing more coverage than Task 283, and alternating selected-suite plus full
V8v7 A/B improvement. Reject and remove if the extra kernel/stencil boundaries lose.

Result: implemented, measured, rejected, and removed. The flat partitioner and bounded
range continuation were correct, all 100 release tests and complete smoke passed, and
the catalog added one shared 136-byte continuation template. It exposed 101 additional
three-bytecode supernodes in Earley-Boyer, three in Richards, and three in Splay beyond
Task 283's exact blocks.

Despite that real coverage, the ten-pair selected-suite comparison in
`reports/task284-property-tiler-selected-ab-10/comparison.txt` regressed Richards 0.56%,
Earley-Boyer 0.33%, and Splay 3.11%; geometric aggregate changed 1729.61 to 1706.40
(-1.34%). The candidate SHA-256 was
`3cec34e0d7f501a5aa6c425b9233918d841ca3e4e6d60d0814dcb03f354d368b`.

The lesson is about composition cost, not legality: a direct atom surrounded by two
generic semantic ranges adds two native/Rust/native boundaries. Closing more atoms does
not help unless adjacent atoms also close, or the whole block is cooked as one template.
Task 283's exact closed block remains accepted; the production tree was restored to its
binary exactly.
