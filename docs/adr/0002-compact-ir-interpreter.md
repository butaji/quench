# ADR 0002: Compact IR + IR interpreter (not walker, not bytecode yet)

- Status: accepted (2026-08-08); scoped by ADR 0003 — this ADR governs the
  near-term Tier-0 interpreter on the ECMAScript execution plane. The
  long-term instruction encoding is compact bytecode per ADR 0003;
  "not bytecode yet" applies to the current milestone only.
- Context: the engine pipeline is documented as `OXC AST -> Quench IR ->
  interpreter` (`docs/architecture.md`), but `QuenchIr`/`IrProgram`
  (`src/ir.rs`) is only an owned `Box<[ast::Statement]>` wrapper — execution
  is still a recursive tree walk over `crate::ast`
  (`eval::eval_statement`/`eval_expression`, giant matches on 21 `Statement`
  + 33 `Expression` variants). Abrupt flow travels through ~20 thread-local
  side channels (`CONTROL_FLOW`, `LABEL_STACK`, generator resume slots, …).
  The user requirement (2026-08-08): a real IR with an **IR interpreter** —
  pc-based execution over index-addressed instructions — explicitly **not**
  a tree walker, and explicitly **not bytecode yet** (no opcode encoding,
  no register allocation). The IR must serve the JS runtime now and grow
  to TS/JSX/TSX later.
- Decision:
  - **IR form.** `src/ir/` module: `IrProgram` owns arenas — `funcs:
    Vec<IrFunction>` (`FuncId(u32)`; entry function = script body),
    `consts` pool, `atoms` (interned identifier/property names), and
    legacy AST side tables (escape hatch, below). `IrFunction` holds
    `code: Box<[Instr]>`, params, flags (strict/arrow/async/generator),
    and a per-function try-handler table `{start_pc, end_pc, catch_pc,
    finally_pc}`. `Instr` is a plain Rust enum of **high-level ops** with
    `u32` operands (`Const`, `LoadName`, `StoreName`, `Binary`, `Jump`,
    `JumpIfFalse`, `Call`, `GetProp`, `MakeFunction(FuncId)`, `Return`,
    `Throw`, `EnterTry`, `LegacyStmt`, `LegacyExpr`, …). This is the
    storage contract from `docs/architecture.md` made concrete: one
    contiguous instruction arena per function, children referenced by
    index, no OXC references, no `Rc` edges.
  - **Interpreter, not walker.** `src/ir/exec.rs` executes with an
    explicit `pc`, an operand stack (`Vec<Value>`), and the existing
    dynamic `Rc<RefCell<Environment>>` name resolution (unchanged —
    slot-based resolution is out of scope). break/continue/return compile
    to `Jump`/`Return` resolved at compile time; try/catch/finally runs
    off the handler table instead of walker recursion.
  - **Compiler.** `src/ir/compile/` lowers `crate::ast` → IR. Function
    bodies compile eagerly into `funcs`. Hoisting semantics of
    `interpreter/helpers.rs` (`hoist_functions`, `predeclare_var`,
    `predeclare_let_const`) are replicated as compiler-emitted prelude
    instructions per function.
  - **Legacy escape hatch (the migration mechanism).** Any construct not
    yet compiled (classes, destructuring, `with`, for-in/of, JSX,
    spreads/getters, …) is emitted as `LegacyStmt`/`LegacyExpr` pointing
    at an owned AST subtree in the side tables; the executor calls the
    existing walker for that subtree and translates any thread-local
    `ControlFlow` back into IR jumps via a flow-scope stack that mirrors
    the compile-time loop/label context. Coverage grows
    construct-by-construct, each step gated by the test262 stage run.
  - **Generators/async stay on the AST replay engine**
    (`value/generator.rs`, `generator_replay.rs`) — compiler emits
    `MakeGenerator` referencing a legacy AST body. Resumable IR frames
    replace replay later, as their own measured step.
  - **Function values** gain `enum FunctionBody { Ast(..), Ir(Rc<IrProgram>,
    FuncId, ..) }` in `ValueFunction`; the Ir variant keeps the original
    `Rc<Vec<Statement>>` alongside for `Function.prototype.toString`.
  - **Dual path during migration.** `Context::eval` compiles + executes IR
    by default; `QUENCH_IR=0` forces the walker for diffing. `eval()` and
    `new Function` stay on the walker until the IR path reaches parity;
    walker deletion is a separate, final step.
  - **Frontend growth.** TS/JSX/TSX already funnel through `crate::ast`
    (TS stripped pre-lower, JSX lowered to `Expression::Jsx*`). Supporting
    them on the IR path means extending `lower/` + the compiler — the IR
    and executor are frontend-agnostic by construction.
- Consequences:
  - Explicit non-goals for this change: no bytecode encoding, no register
    allocation, no variable slot resolution, no JIT metadata (consistent
    with `docs/architecture.md`'s rejection of speculative machinery); no
    generator/async IR migration; no NaN boxing / bumpalo / interner work
    (R19–R21 stay queued separately).
  - The `CONTROL_FLOW`/`LABEL_STACK` thread-locals shrink to legacy-interop
    duty only and are deleted with the walker.
  - Milestones: M0 IR skeleton → M1 compiler core → M2 executor → M3
    `Context::eval` wiring (stage-gate green) → M4+ coverage expansion
    (patterns, for-in/of, classes, JSX, resumable generator frames,
    `eval`/`new Function` cutover, walker deletion). Each milestone ends
    with `cargo test -p quench-runtime`, clippy `-D warnings`, and the
    current test262 stage non-regression.
