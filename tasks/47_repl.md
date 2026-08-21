# repl

Bun classifies the REPL as mostly implemented: `bun --interactive` provides a
Node-compatible interactive REPL. Known gaps are missing result previews,
incomplete tab completion for `let`/`const`/`class`, and V8-specific wording
differences. Quench currently provides `start()`/`REPLServer` lifecycle,
prompt, and context hooks; interactive evaluation remains host-dependent.
Validate the exposed surface with focused and applicable upstream Node API
tests rather than treating the lifecycle shape as full REPL compatibility.
