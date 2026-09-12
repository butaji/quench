# 366 — Symbolic PC and edge-only site materialization

Status: planned

Eliminate the serialized physical `InlineSite`/instruction-pointer update from closed native
composites. Extend the connector context with one explicit state:

- `PcState::Known(BytecodeOffset)` while a quoted region and its patched successors determine
  the exact guest position; or
- `PcState::Materialized(InlineSitePtr)` at an edge whose kernel requires the physical site.

Internal sequential edges and loop backedges patch direct machine targets and carry the
known PC as compiler data. Do not increment or reload a physical site pointer after every
leaf. Insert one generated materialization morphism only before a slow/effect kernel,
exception/handler edge, dynamic call requiring site metadata, or region exit. Its record
must identify the exact unexecuted/failing bytecode, accumulator, live values and ownership
state required by Task 164; a failing operation may never report its successor PC.

This is a categorical context extension, not a second control-flow representation. The
micro-op quote owns guest PCs and labels; the two-pass linker derives byte addresses and
materialization records. `Kernel` and `StencilInstance` consume the same typed `PcState`
transitions, and named constants define all byte offsets and encoding ranges.

First experiment: combine with Tasks 309/157/316/348 on one mixed numeric/property block
and one loop. Counters report executed physical site advances, exact-PC materializations,
direct internal branches and materialization reasons. Linked disassembly must contain no
per-leaf `site_advance`; deliberate slow, throw and guard-failure tests must resume or report
at the exact bytecode. Complete V8v7 correctness and alternating A/B decide enablement.

Primary source: Ertl and Melançon, “Optimizing Virtual Machine Instruction Pointer Updates,”
ECOOP 2024
<https://drops.dagstuhl.de/storage/00lipics/lipics-vol313-ecoop2024/LIPIcs.ECOOP.2024.14/LIPIcs.ECOOP.2024.14.pdf>.
