# ADR 0003: Superseded — two-plane TypeGraph architecture

- Status: superseded by ADR 0005.

The separate persistent TypeGraph and execution plane are not planned. Static
and runtime knowledge share the single `ProgramDb` fact algebra described in
ADR 0005.
