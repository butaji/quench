# 03 — Stencil-only execution policy and coverage

Status: in_progress

The stencil mode has no hotness threshold: supported functions compile on first use and execute through linked stencil code. The interpreter is a diagnostic failure path in stencil mode, making missing lowering visible instead of silently hiding it.

Done: first-use compilation, dynamic bytecode execution, native numeric entry paths, loop accounting, and explicit guard-failure errors.

Remaining: eliminate helper-dominated execution for ordinary dynamic bytecodes; ensure every supported V8v7 source line maps to a stencil/kernel path; verify no normal statement falls through to AST interpretation.
