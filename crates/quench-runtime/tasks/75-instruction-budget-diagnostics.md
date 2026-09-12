# 75 — Instruction-budget diagnostics for stencil nontermination

Status: complete

Add disabled-by-default `DEEGEN_INSTRUCTION_BUDGET`. The generic block kernel consumes the budget and exits through the ordinary stencil error edge when exhausted, allowing coverage and structured opcode statistics to be written instead of losing all evidence to an external timeout. Structured JIT statistics are emitted before a terminal error when requested.

Evidence: a two-million-instruction Earley diagnostic produced `reports/earley-k1-budget.jsonl` and identified lines 813–815 (`sc_length`) as 98% of recorded execution. That evidence led directly to [[74-var-binding-semantics]].

The option does not participate in normal execution and has no benchmark-source matching or hotness policy.

