# 01 — Composable stencil category core

Status: complete

The canonical representation keeps immutable stencil descriptions separate from final emission. Sequencing is associative, empty is the identity, holes and labels remain data until final linking, and higher-level blocks are compositions of the same representation rather than a separate compiler path.

Kernels are immutable shared code images. A `StencilTemplate` has patch obligations; linking produces a `StencilInstance`. Kernels and linked instances expose compatible entry/exit contracts, so either can participate in higher-level compositions. Shared kernels and reusable instances are referenced rather than recopied.

Constraint: effects occur only at the link/execute edge. Do not conflate composition with mutation of executable memory.
