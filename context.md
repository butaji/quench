# Quench Runtime Interpreter Scout Findings

## Files Retrieved

### 1. `crates/quench-runtime/src/interpreter.rs` (lines 1-248)
- Entry point for JavaScript evaluation
- Defines `eval_program()` which handles hoisting and executes statements
- Thread-local recursion depth tracking via `CURRENT_DEPTH`, `MAX_RECURSION_DEPTH_OVERRIDE`
- Control flow handling for `break`/`continue` via `CONTROL_FLOW` thread-local
- `this` binding management via `CURRENT_THIS` thread-local
- Super class tracking via `SUPER_CLASS` thread-local

### 2. `crates/quench-runtime/src/eval/mod.rs` (lines 1-25)
- Exports evaluation functions from submodules
- Key exports: `eval_expression`, `call_value`, `call_value_with_this`, `call_js_function_with_this`

### 3. `crates/quench-runtime/src/eval/function.rs` (lines 1-142)
- **Function call implementation** - core of how functions execute
- `call_value_with_this()` - main entry point for all function calls
- `call_js_function_impl()` - handles JS function calls:
  - Creates new `Environment::with_parent()` closure environment
  - Binds `this` to the new scope
  - Defines parameters from args (undefined for missing)
  - Creates `arguments` object for non-arrow functions
  - Predeclares `let`/`const` variables
  - Calls `eval_statements()` for body or `call_arrow_body()` for arrow functions

### 4. `crates/quench-runtime/src/eval/statement.rs` (lines 1-185)
- `eval_statement()` - matches on Statement enum, evaluates each type
- `eval_statements()` - iterates statements, checks for control flow breaks
- Block evaluation (`eval_block()`) - pushes/pops scope
- Loop evaluation with iteration limits and control flow handling

### 5. `crates/quench-runtime/src/callframe.rs` (lines 1-120)
- **Explicit call frame stack** for HIR/trampoline interpreter
- `CallFrame` struct: locals (register storage), block, pc, this, caller, return_slot
- `FrameStack` - explicit Vec-based frame management (NOT used by AST interpreter)
- This is for a potential future HIR-based interpreter, separate from the current AST interpreter

### 6. `crates/quench-runtime/src/value/function.rs` (lines 1-208)
- `ValueFunction` - JS function representation with closure, params, body
- `is_arrow` field distinguishes arrow functions
- `arrow_body` stores expression or block body
- `proto_cell` - lazily-initialized prototype cache
- `NativeFunction` / `NativeConstructor` - host-provided functions

### 7. `crates/quench-runtime/src/env.rs` (lines 1-270)
- **Environment (scope chain)** implementation
- `Scope` struct: HashMap bindings, declarations (TDZ tracking), var_kinds, this_value
- `Environment` struct: Vec<Scope>, parent (Rc<RefCell<Environment>>)
- Lexical lookup via scope chain traversal
- `push_scope()` / `pop_scope()` for block scope management

### 8. `crates/quench-runtime/src/ast.rs` (lines 1-200)
- AST types for all JS constructs
- **NO async/generator variants** in Expression enum
- No `Yield`, `Await`, `AsyncFunction` in the runtime AST

### 9. `crates/quench-runtime/src/lower/expr.rs` (lines 23-135)
- `lower_yield_expr()` - parses yield but returns `Undefined` (not actual yield)
- `Await` - just lowers to the argument expression (no await handling)
- These are stubbed/silently dropped during lowering

### 10. `crates/quench-runtime/src/test262/skip.rs` (lines 1-90)
- Confirms **generators**, **async-functions**, **async-iteration**, **Promise** are skipped
- Lists ~40 unsupported features

---

## Key Code

### Function Call Flow (eval/function.rs:44-77)
```rust
fn call_js_function_impl(
    f: ValueFunction,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    let closure = Rc::clone(&f.closure);
    let params = f.params.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env.current_scope_mut().set_this(this_val);
    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }
    // Create arguments object for non-arrow functions
    if !f.is_arrow {
        let args_obj = create_arguments_object(&f, args);
        call_env.define("arguments".to_string(), args_obj);
        predeclare_var(&f.body, &mut call_env);
        predeclare_let_const(&f.body, &mut call_env);
    }
    let call_env = Rc::new(RefCell::new(call_env));
    if f.is_arrow {
        call_arrow_body(&f, &call_env)
    } else {
        eval_statements(&f.body, &call_env, false)
    }
}
```

### Call Stack Implementation (thread-local based)
- `CURRENT_DEPTH` - thread-local usize tracking recursion depth
- `check_depth()` / `release_depth()` - manage depth for 10k max limit
- Environment chain provides lexical scope, NOT a call stack structure
- Closures capture parent environment via `Rc<RefCell<Environment>>`
- The explicit `FrameStack` in `callframe.rs` is **NOT used** by the AST interpreter

### AST Expression Types (crates/quench-runtime/src/ast.rs)
```rust
pub enum Expression {
    // ... no AsyncFunction, Yield, Await, Generator ...
    FunctionExpression { name, params, body },
    ArrowFunction { params, body },
    // ...
}
```

---

## Architecture

### Current Interpreter (AST-based, recursive)
1. **Parse** → SWC → Custom AST (via `lower` module)
2. **Hoisting** → `hoist_functions()`, `hoist_classes()`, `predeclare_let_const()`
3. **Execute** → `eval_program()` → `eval_statements()` → `eval_statement()` → `eval_expression()`
4. **Function calls** → `call_value_with_this()` → `call_js_function_impl()` → new Environment + `eval_statements()`
5. **Return** → unwinds via Rust call stack (depth-limited to 10k)

### Call Stack Model
- **Implicit** - relies on Rust's native call stack via recursive function calls
- Thread-local depth counter prevents stack overflow (10k limit)
- Closures capture environment chain (parent pointers), not explicit frames

### Alternative (Future): HIR/Trampoline Interpreter
- `callframe.rs` provides `CallFrame` and `FrameStack` for explicit frame management
- Would use `FrameId` handles instead of Rust recursion
- **Currently not integrated** - separate infrastructure

---

## Async/Generator Support Status

### What's Missing
1. **AST nodes** - No `Yield`, `Await`, `AsyncFunction` in Expression enum
2. **Runtime support** - No generator state machine, no async/await transformation
3. **Promises** - `Promise` built-in exists but is in `skip.rs` for test262
4. **Microtask queue** - `context/microtask.rs` exists but only partially integrated

### What's Stubbed
- `lower_yield_expr()` returns `Undefined` silently
- `Await` expression just evaluates its argument
- test262 skips async/generator features entirely

### Implemented Parts
- Promise object kind (`ObjectKind::Promise`)
- Promise state enum (`PromiseState::Pending/Fulfilled/Rejected`)
- Promise built-in constructor and methods
- Microtask queue structure

---

## Start Here

**First file to open:** `crates/quench-runtime/src/eval/function.rs`

**Why:** This is the heart of function execution. Understanding `call_js_function_impl()` shows:
1. How closures capture environments
2. How `this` binding works
3. How arguments object is created
4. The distinction between regular functions and arrow functions
5. The recursion depth management

**Second file:** `crates/quench-runtime/src/env.rs` - to understand scope chain lookup

**Third file:** `crates/quench-runtime/src/eval/statement.rs` - to understand execution flow

---

## Supervisor Coordination

No supervisor contact needed. This is a complete scout of the interpreter architecture with no blockers.

---

## Acceptance Report

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Scouted interpreter.rs and eval/ directory; documented execution flow, function calls, call stack model, and async/generator gaps. Output written to /Users/admin/Code/GitHub/quench/context.md"
    }
  ],
  "changedFiles": [],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "find crates/quench-runtime/src/eval -name '*.rs' | xargs ls -la",
      "result": "passed",
      "summary": "Listed eval module files"
    }
  ],
  "validationOutput": [
    "Read 9 source files totaling ~1200 lines",
    "grep for async/generator/yield returned 30+ matches confirming skip status",
    "All key structures identified and documented"
  ],
  "residualRisks": [
    "The explicit FrameStack in callframe.rs is documented but not used - future work",
    "Async/generator support is stubbed, not implemented - documented in skip.rs"
  ],
  "noStagedFiles": true,
  "diffSummary": "Read-only scout - no changes made",
  "reviewFindings": [
    "info: interpreter.rs:1-248 - Entry point with thread-local depth/this/control-flow tracking",
    "info: eval/function.rs:44-77 - Core function call implementation",
    "info: env.rs - Scope chain with TDZ support for let/const",
    "info: callframe.rs - Explicit frames for future HIR interpreter (not used)",
    "info: ast.rs - No async/generator AST variants (documented in skip.rs)",
    "info: lower/expr.rs:118-122 - yield/await stubbed to return Undefined",
    "no blockers"
  ],
  "manualNotes": "The interpreter uses recursive Rust calls (implicit stack) with 10k depth limit. The explicit FrameStack infrastructure exists but is for a potential future HIR interpreter, not currently integrated."
}
```
