# Quench High-level IR (HIR) Design

## 1. Purpose

The HIR is the runtime-facing intermediate representation for JavaScript, TypeScript, JSX, and TSX code. It sits between the swc-derived source AST and the execution engine. Its goals are:

1. **Capture JS semantics faithfully** — every observable ECMAScript behavior must be representable.
2. **Erase TS/JSX surface syntax** — types, type assertions, JSX tags, and TS-only declarations disappear before HIR.
3. **Simplify the runtime** — the interpreter should not re-implement scoping, destructuring, optional chaining, or class construction on every evaluation.
4. **Stay efficient** — prefer compact indices and arena storage over HashMap lookups and string re-computation.

The HIR is *not* a bytecode. It is a structured IR with named operands and basic-block-style control flow. It can be executed directly by a trampoline interpreter or lowered to a stack/ register bytecode later.

## 2. Design principles

- **Resolved bindings.** Every variable reference is a `Local`, `Global`, `Upvalue`, `This`, or `Import` slot. No runtime name lookup in the hot path.
- **Explicit control flow.** Loops, `break`/`continue`, `switch`, optional chaining, logical operators, and exceptions become labeled jumps or exception edges.
- **Uniform values.** One `Value` type for the interpreter; objects, functions, and builtins share a single object model.
- **TypeScript as an accelerator.** Type annotations and inferred types are preserved through lowering to specialize operations, choose shapes, and avoid boxing. Optimizations are guarded so unsound TS types do not break JS semantics.
- **Arena allocation.** HIR nodes, strings, and object shapes live in a `bumpalo` arena to avoid per-node allocations.
- **Incremental adoption.** The first milestone is a trampoline HIR interpreter that replaces the recursive tree-walker without requiring a full compiler rewrite.

## 3. Pipeline

```text
.ts/.tsx/.js/.jsx source
        │
        ▼
   swc parser + JSX transform + import/export strip
        │
        ▼
   Source AST (Quench AST)
        │
        ▼
   HIR builder
   ├─ resolve scopes and bindings
   ├─ extract TypeScript types from annotations & inference
   ├─ lower destructuring, rest, default params
   ├─ lower optional chaining / logical ops to control flow
   ├─ build class tables
   ├─ compute exception tables
   └─ specialize ops based on type information
        │
        ▼
   HIR
        │
        ▼
   Trampoline interpreter  (first milestone)
        │
        ▼
   Bytecode VM  (future milestone)
```

## 4. HIR units

### 4.1 `Module`

```rust
struct Module<'a> {
    /// Arena-owned strings and nodes.
    arena: &'a bumpalo::Bump,
    /// Top-level function. A script becomes a function with no parameters.
    root: FunctionIdx,
    /// Imported bindings (for when modules are implemented).
    imports: &'a [Import],
    /// Exported names and their local slots.
    exports: &'a [Export],
    /// Interned string table for property names and identifiers.
    strings: StringInterner,
}
```

### 4.2 `Function`

```rust
struct Function<'a> {
    /// Number of local slots reserved for parameters + locals.
    param_count: u32,
    local_count: u32,
    /// true if the last parameter is a rest binding.
    has_rest: bool,
    /// Default-value expressions for parameters, indexed by param slot.
    defaults: &'a [Option<ExprIdx>],
    /// Explicit `this` behavior: arrow functions ignore this.
    this_mode: ThisMode, // Lexical | Global | Strict
    /// Upvalues captured from enclosing scopes.
    upvalues: &'a [Upvalue],
    /// Basic blocks.
    blocks: &'a [Block],
    /// Entry block.
    entry: BlockIdx,
    /// Exception table: ranges of blocks → handler blocks.
    exception_table: &'a [ExceptionRange],
}
```

### 4.3 `Block`

A basic block is a sequence of operations with a single terminator.

```rust
struct Block {
    ops: Vec<Op>,
    term: Terminator,
}

enum Terminator {
    Return(Option<ValueIdx>),
    Jump(BlockIdx),
    JumpIf { cond: ValueIdx, then_: BlockIdx, else_: BlockIdx },
    Throw(ValueIdx),
    Yield / Await (reserved for future generators/async),
}
```

## 5. HIR operands and values

### 5.1 Operands

Most operations consume and produce `ValueIdx`, an index into the current function's value stack or local array.

```rust
type ValueIdx = u32;

enum Operand {
    Local(LocalIdx),
    Temp(ValueIdx),      // SSA-like temporary in the current evaluation frame
    Constant(ConstIdx),
    Upvalue(u32),
    Global(Symbol),
    This,
    NewTarget,
    Import(ImportIdx),
}
```

### 5.2 Constants

Constants are interned per module:

```rust
enum Constant {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Symbol),   // interned
    Regex(Symbol, Symbol), // pattern + flags (future)
    BigInt(Symbol),   // future
}
```

## 6. HIR operations

A small, orthogonal instruction set is enough to express all JS semantics.

### 6.1 Literals and locals

```rust
enum Op {
    LoadConst { dst: ValueIdx, c: ConstIdx },
    Move { dst: ValueIdx, src: Operand },
    LoadGlobal { dst: ValueIdx, name: Symbol },
    StoreGlobal { name: Symbol, src: ValueIdx },
    LoadUpvalue { dst: ValueIdx, idx: u32 },
    StoreUpvalue { idx: u32, src: ValueIdx },
}
```

### 6.2 Object / array construction

```rust
enum Op {
    NewObject { dst: ValueIdx, shape: ShapeIdx },
    NewArray { dst: ValueIdx, len: u32 },
    SetProp { obj: ValueIdx, key: PropKey, val: ValueIdx },
    GetProp { dst: ValueIdx, obj: ValueIdx, key: PropKey },
    DefineMethod { obj: ValueIdx, key: PropKey, func: FunctionIdx },
    DefineGetter { obj: ValueIdx, key: PropKey, func: FunctionIdx },
    DefineSetter { obj: ValueIdx, key: PropKey, func: FunctionIdx },
}
```

`PropKey` is either an interned string symbol or a computed `ValueIdx`.

### 6.3 Calls

```rust
enum Op {
    Call {
        dst: ValueIdx,
        callee: ValueIdx,
        this: Operand,
        args: &'a [ValueIdx],
        spread: Option<u32>, // index of spread argument, if any
    },
    New {
        dst: ValueIdx,
        ctor: ValueIdx,
        args: &'a [ValueIdx],
        spread: Option<u32>,
    },
    SuperCall { args: &'a [ValueIdx] },
    BindThis { dst: ValueIdx, func: ValueIdx, this: Operand },
}
```

The `this` operand removes the ad-hoc method-binding logic currently in `properties.rs`.

### 6.4 Operators

All unary, binary, update, and assignment operators become explicit ops with a result destination.

```rust
enum Op {
    Unary { dst: ValueIdx, op: UnaryOp, arg: ValueIdx },
    Binary { dst: ValueIdx, op: BinaryOp, left: ValueIdx, right: ValueIdx },
    IncDec { dst: ValueIdx, op: UpdateOp, arg: ValueIdx },
    Assign { dst: ValueIdx, src: ValueIdx }, // for simple assignments
}
```

Compound assignments (`+=`, `||=`, etc.) desugar to `GetProp`/`Binary`/`SetProp` sequences.

### 6.5 Destructuring

Destructuring is lowered to explicit field/index reads and local stores. No special interpreter node is needed.

```text
let {a, b: [c]} = obj;
  -> t0 = obj
     a  = GetProp(t0, "a")
     t1 = GetProp(t0, "b")
     c  = GetProp(t1, 0)
```

Rest elements become `CopyRest(t0, start_idx, dst)`.

### 6.6 Class construction

A class is built once at HIR-generation time:

```rust
struct ClassTemplate {
    constructor: Option<FunctionIdx>,
    extends: Option<FunctionIdx>,
    prototype_methods: &'a [(Symbol, FunctionIdx)],
    static_methods: &'a [(Symbol, FunctionIdx)],
    instance_fields: &'a [FieldInit],
    static_fields: &'a [FieldInit],
}
```

At runtime:

```rust
Op::BuildClass { dst, template: ClassIdx }
```

creates the constructor function and prototype object from the template.

## 7. Control flow

### 7.1 Conditionals and loops

`if`, `while`, `for`, and `do-while` become basic blocks with `JumpIf`/`Jump` terminators. `break` and `continue` target labels are resolved to block indices at HIR build time.

### 7.2 Optional chaining and logical operators

Optional chaining and short-circuiting operators are lowered to explicit branches:

```text
a?.b.c
  -> t0 = a
     if t0 == null/undefined jump Lnull
     t1 = GetProp(t0, "b")
     t2 = GetProp(t1, "c")
     jump Ldone
  Lnull:
     t2 = undefined
  Ldone:
```

This removes the special `OptChainMember`/`OptChainCall` interpreter cases.

### 7.3 `switch`

`switch` desugars to a sequence of `Binary ===` comparisons and jumps. Each `case` is a labeled block; `default` is the fall-through target.

## 8. Exception handling

The HIR uses an **exception table** instead of thread-local flags.

```rust
struct ExceptionRange {
    /// Block range covered by this handler.
    start_block: BlockIdx,
    end_block: BlockIdx,
    /// Block to enter when an exception is thrown in the range.
    handler_block: BlockIdx,
    /// Local slot that receives the caught value.
    catch_slot: LocalIdx,
    /// Block to run after catch (finally), if any.
    finally_block: Option<BlockIdx>,
}
```

The trampoline unwinds frames and searches the exception table of the current function; if no handler is found, it continues unwinding. `finally` is implemented by duplicating the finally block at every exit path (return, break, continue, throw) — standard lowering — so the runtime only needs try/catch edges.

## 9. Variable resolution and closures

### 9.1 Scopes

The HIR builder performs a single scope-analysis pass over the source AST:

- Every `var`/`let`/`const`/`function`/`class` gets a `LocalIdx`.
- Variables used in nested functions become `Upvalue`s.
- Free variables that are not captured resolve to `Global`.
- Arrow functions use `ThisMode::Lexical` and capture `This` from the enclosing non-arrow function.

### 9.2 Upvalues

```rust
enum Upvalue {
    /// Captures a local in the immediately enclosing function.
    Local(LocalIdx),
    /// Captures an upvalue from the enclosing function.
    Upvalue(u32),
}
```

At runtime, an `Upvalue` object is an `Rc<RefCell<Value>>` shared between the closure and the enclosing scope. Mutations propagate correctly, fixing the current snapshot-copy bug.

### 9.3 Global object

Global declarations (`var` at script level, function declarations) are bound to the global object via `StoreGlobal`. The global object is a normal `Value::Object`.

## 10. Function calls and frames

The trampoline interpreter uses an explicit call stack of `CallFrame`s:

```rust
struct CallFrame<'a> {
    function: &'a Function<'a>,
    /// Local slots: params + locals + temporaries.
    locals: Vec<Value>,
    /// Upvalue references.
    upvalues: Rc<[UpvalueRef]>,
    /// Current block and instruction index.
    pc: (BlockIdx, usize),
    /// Return destination (frame + ValueIdx) in the caller.
    return_to: ReturnTarget,
    /// This binding for the call.
    this: Value,
    /// Home object for `super`.
    home_object: Option<Value>,
}
```

Calling a function:

1. Allocate a frame with `param_count + local_count + temp_count` slots.
2. Bind positional arguments; fill missing params with `undefined` or default expressions.
3. Build the rest array if `has_rest`.
4. Push the frame and resume the trampoline loop.

This removes the native-stack recursion limit and enables a true `MAX_JS_STACK` guard, tail-call optimization, and `async`/`await` suspension later.

## 11. Object model simplifications

### 11.1 Object representation

```rust
struct Object {
    /// Shape determines property layout.
    shape: Shape,
    /// Dense property storage indexed by shape slot.
    properties: Vec<Value>,
    /// Optional dense array elements.
    elements: Vec<Value>,
    /// Prototype object.
    prototype: Option<Gc<Object>>,
    /// Internal kind flags.
    kind: ObjectKind,
}
```

### 11.2 Shapes

Each distinct property layout is a `Shape` with a stable symbol→slot map. Shapes are interned per realm. Property lookup becomes:

```rust
obj.shape.lookup(key) -> Option<SlotIdx>
```

This replaces the per-access `HashMap` lookup and makes inline caching straightforward in the future.

### 11.3 Functions

```rust
struct FunctionObject {
    /// HIR function template.
    hir: FunctionIdx,
    /// Captured upvalues.
    upvalues: Rc<[UpvalueRef]>,
    /// `this` mode and strictness.
    mode: FunctionMode,
    /// Pre-built prototype object for constructors.
    prototype: Option<Gc<Object>>,
    /// For bound functions: bound this and partial args.
    bound: Option<BoundData>,
}
```

`NativeFunction` and `NativeConstructor` are unified as host functions whose bodies are Rust closures taking a `&mut Context` and argument slice.

## 12. TypeScript-aware HIR optimizations

TypeScript types are not erased blindly. The HIR builder extracts type information from annotations, `as const`, literal types, enum declarations, and simple inference, then uses it to emit faster, more specialized operations. Because TypeScript types are unsound, every specialization that could diverge from JS semantics is paired with a **runtime guard** or a **deoptimization path**.

### 12.1 Type lattice

```rust
enum Ty {
    Top,          // unknown / any / unconstrained
    Bottom,       // never / unreachable
    Undefined,
    Null,
    Bool,
    Number,
    BigInt,
    String,
    Symbol,
    Literal(ConstIdx),       // literal string/number/boolean
    Object(ObjectShapeIdx),
    Array(Box<Ty>),
    Tuple(Vec<Ty>),
    Function(FunctionSigIdx),
    Class(ClassIdx),
    Union(Vec<Ty>),
    Intersection(Vec<Ty>),
}
```

`Top` means "no useful static information" and produces generic ops. `Bottom` lets the optimizer eliminate code. `Object` carries a shape index when the structure is known.

### 12.2 Where type information comes from

- **Parameter and variable annotations**: `let n: number = 0` gives `n` type `Number`.
- **Return-type annotations**: function bodies can be specialized for the declared return type.
- **`as const` and literal types**: `const p = { x: 1, y: 2 } as const` yields a shape with fixed numeric slots.
- **Enums**: numeric enums become constant tables; string enums become interned strings.
- **Class fields**: typed fields define a fixed object shape.
- **Interface/type alias declarations**: provide shapes even for values declared elsewhere.
- **Simple local inference**: `const x = 1 + 2` is inferred as `Literal(3)` if both operands are constant.

### 12.3 Type-specialized operations

The HIR uses the operand type to choose the concrete operation:

| Source operation | Typed HIR op | When emitted |
|---|---|---|
| `a + b` | `AddNumber` | both operands are `Number` |
| `a + b` | `AddString` | both operands are `String` |
| `a + b` | `Add` (generic) | otherwise |
| `a.b` | `GetPropMono { slot }` | `a` has an `Object` shape with known key `b` |
| `a.b = c` | `SetPropMono { slot }` | same |
| `a.b()` | `CallMethodMono { shape, slot, sig }` | `a` shape and method signature known |
| `f(a, b)` | `CallDirect { func, sig }` | callee is a known function with matching signature |
| `a === b` | `StrictEqNumber`, `StrictEqString`, `StrictEqBool` | operand types match and are primitive |
| `a[i]` | `GetElementDense` | `a` is `Array(T)` and `i` is `Number` |

### 12.4 Guarded specialization

When the static type is narrower than the runtime type can be, the HIR emits a guard:

```text
op: TypeGuard { val: a, expected: Number, ok: B_fast, fail: B_slow }
B_fast:
  result = AddNumber(a, b)
  jump B_done
B_slow:
  result = Add(a, b)   // generic JS add
  jump B_done
B_done:
```

Guards are cheap when the type is stable (the common case) and keep the interpreter correct when TypeScript lies.

### 12.5 Boxing strategy

- **Known primitives** (`number`, `boolean`, `string`, `symbol`) can live in unboxed locals when their type is stable across the whole function. A `Box` op converts to `Value` at boundaries (upvalue capture, generic call, property store).
- **Known objects and arrays** keep their object pointer and shape; property access uses slot indices.
- **Generic locals** remain full `Value` enums.

This is especially profitable for numeric loops, vector math, and React-style props objects with known shapes.

### 12.6 Shape-driven object layout

TypeScript interfaces and object literal types directly produce object shapes:

```ts
interface Point { x: number; y: number }
```

generates a shape `{ x: slot 0, y: slot 1 }`. Every `Point` value created at runtime uses that shape, so `p.x` becomes a single array index load plus a shape check.

For optional properties (`x?: number`), the shape reserves the slot and stores `undefined` when absent. For union shapes (`{a}|{b}`), the HIR falls back to generic property access or emits a shape-switch guard.

### 12.7 Function specialization

- **Monomorphic call sites**: if a call expression has a callee of a known function type, emit `CallDirect` with the resolved `FunctionIdx` and validated argument count/types.
- **Generic / overloaded functions**: emit `Call` and rely on runtime dispatch.
- **Small function inlining**: functions marked `inline` or with a small body and a monomorphic call site can be inlined into the caller's HIR.

### 12.8 Type-driven dead code elimination

- A branch conditioned on a value of type `never` is unreachable.
- A function declared `never` returns can omit normal return paths.
- `if (x)` where `x` is typed `true` or `false` can constant-fold the branch.

### 12.9 TS-only constructs still erased

Types used only for checking are still removed from runtime:

- Type annotations themselves become `Ty` metadata, not runtime values.
- `interface`, `type alias`, and pure-type declarations produce shapes/types but no runtime objects.
- Type assertions (`as`, `!`, `satisfies`) become the underlying expression plus a possible guard.
- JSX tags are desugared before HIR building.

## 13. TypeScript and JSX erasure

Runtime-irrelevant TypeScript and JSX surface syntax is removed before or during HIR construction:

- Type annotations are converted to `Ty` metadata.
- `interface`, `type alias`, `declare`, `TsModule`, and pure-type items contribute shapes/types but no runtime code.
- Type assertions become the underlying expression, optionally wrapped in a `TypeGuard`.
- JSX is transformed to `ink.createElement(...)` calls before HIR building.

The HIR therefore represents only runtime JS semantics, but it is **guided by** TypeScript types to choose efficient representations.

## 14. Modules (future)

When native ES modules are implemented, the HIR represents them as:

```rust
struct Import {
    source: Symbol,
    kind: ImportKind, // Default, Named, Namespace, SideEffect
    local: LocalIdx,
}

struct Export {
    name: Symbol,
    local: LocalIdx,
}
```

Module loading produces a `ModuleRecord` with resolved exports; the runtime links modules before execution.

## 15. Implementation strategy — quick wins first, no stubs

Each phase below is ordered by effort vs. payoff. If a feature needed by the phase is not implemented, the runtime must throw a clear error; stubs and silent fallbacks are not allowed.

1. **Fix runtime correctness quick wins** (Tasks 250, 253, 91, 97, 147, 191, etc.)
   - Preserve thrown values, load real test262 harness, tighten skip lists, fix small spec bugs.
   - These unblock accurate measurement and remove silent failures before the HIR rewrite.

2. **Value model + shape foundation**
   - Introduce `Object`, `Shape`, `StringInterner`, and a unified `FunctionObject`.
   - Keep the recursive interpreter running; only replace the value/containers layer.
   - Any missing shape/lookup behavior must throw, not default to a HashMap path.

3. **Explicit call stack (trampoline)** (Task 85)
   - Replace recursive interpreter calls with `CallFrame` + trampoline loop.
   - No-op or stub frame handling is forbidden; unsupported call modes panic.

4. **HIR builder (untyped)**
   - Build HIR from the source AST alongside the recursive interpreter.
   - Run HIR-only smoke tests; every unlowered AST node causes a compile-time panic in the builder.

5. **Type extractor + typed HIR**
   - Collect TS annotations and inferred types.
   - Emit type-specialized ops with guards; fallback ops must be fully implemented.

6. **Switch execution to typed HIR interpreter**
   - Run the existing suite through the HIR interpreter.
   - Every unsupported HIR op throws at runtime until implemented.

7. **Retire recursive interpreter**
   - Remove the old evaluator once the HIR interpreter matches it.

8. **Bytecode VM** (future)
   - Lower HIR to bytecode when profiling justifies it.

## 16. Migration path (summary)

1. **HIR builder** — implement a second lowering pass from source AST to HIR, keeping the recursive interpreter untouched initially.
2. **Type extractor** — collect annotations and simple inferred types, attaching `Ty` to HIR operands.
3. **Value/frame refactor** — introduce the simplified object model, shapes, and explicit `CallFrame`.
4. **Typed trampoline HIR interpreter** — replace `eval_program` with a loop over `CallFrame`s that uses specialized ops and guards.
5. **Retire recursive interpreter** — once the HIR interpreter passes the existing test suite.
6. **Bytecode VM** — lower HIR to bytecode when profiling shows it is worthwhile.

## 16. Efficiency notes

- **No HashMap in hot paths.** Scopes use slot arrays; shapes use slot arrays; strings are interned.
- **Precomputed metadata.** Class tables, exception ranges, default-param expressions, and type-derived shapes are built once.
- **Arena allocation.** HIR nodes, shape tables, and interned strings are bump-allocated.
- **Contiguous value arrays.** Each frame stores locals in a single `Vec<Value>`, friendly to cache.
- **Unboxed locals for known primitives.** Numbers/booleans/strings avoid `Value` boxing inside typed functions.
- **Control flow is data.** Branch targets are block indices, not string labels or pattern matches.

## 17. Open questions

- Should the HIR use basic blocks with φ-nodes for merges, or keep a simpler linear-block form with explicit temporaries?
- Should `Value` use NaN boxing (Task 210) before or after the HIR interpreter is stable?
- How should host functions expose GC-safe references to the JS heap?
- Should the first milestone support `with`, `eval`, and `arguments`, or defer them?
- How aggressive should type-driven specialization be given TypeScript's unsoundness — guard every specialization, or only guard at module boundaries?
