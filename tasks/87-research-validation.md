> **Record of online research used to validate the project's direction.**

# Task 87: Research validation

## Goal

Confirm that the current architecture and task priorities are aligned with how successful JS engines and conformance harnesses are built.

## Sources

- QuickJS internals and stack-overflow discussions (`bellard.org/quickjs`, GitHub issues).
- Boa engine architecture (`boa-dev/boa`, docs.rs/boa_engine).
- Trampoline / stackless interpreter design (academic paper, MDN, Stack Overflow).
- test262 harness implementations (`tc39/test262-harness`, `lahma/test262-harness-dotnet`, SpiderMonkey `jstests.py`).
- JavaScript engine zoo / conformance tracking (`ivankra/javascript-zoo`, `test262.fyi`).

## Key findings

1. **Explicit stack / trampoline is canonical.**
   - QuickJS, Boa, SpiderMonkey, and V8 Ignition all avoid deep native recursion for JS execution.
   - A heap-allocated `Vec<CallFrame>` + loop eliminates stack overflow by construction and makes generators/async straightforward later.
   - This validates Task 85 (trampoline interpreter) as the right fix for the current top blocker.

2. **AST → bytecode is the correctness/performance bridge, not a pure tree-walker.**
   - Boa compiles AST to bytecode and passes >90% of test262.
   - A pure AST interpreter is acceptable for correctness work but should evolve toward bytecode or a register-based VM after stabilization.
   - Our plan to keep a high-level HIR that can feed a future bytecode/AOT backend is correct.

3. **test262 harnesses load helper files from the submodule.**
   - `assert.js`, `sta.js`, `compareArray.js`, `propertyHelper.js`, etc. are executed by the engine, not stubbed.
   - Loading real harness files will remove many false failures and improve trust in test262 numbers.

4. **Value model can remain simple initially.**
   - Both Boa and QuickJS start with an explicit `JsValue` enum and reference counting.
   - NaN-boxing, shapes, and inline caches are performance optimizations, not prerequisites for conformance.

5. **Conformance-driven development is standard.**
   - Major engines run test262 continuously and use failures as a prioritized backlog.
   - Our harness reporting + bucket-fixing workflow matches this practice.

## Conclusion

The current direction is sound:

- Fix stack overflow with a trampoline interpreter (Task 85).
- Load real test262 harness files.
- Use conformance reports to drive correctness fixes with regression tests.
- Defer NaN-boxing / shapes / AOT until correctness and stability are solid.

## Status

`completed`.
