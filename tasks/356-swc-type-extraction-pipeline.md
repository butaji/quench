# Task 356: SWC Type Extraction Pipeline

## Status: BACKLOG

## Goal

Harvest TypeScript type annotations from SWC's AST at parse time and use them to build typed shadow tree nodes that execute with fast-path dispatch and speculative optimization.

## Motivation

SWC preserves TsType annotations in its AST (TsTypeAnn, TsKeywordType, TsTypeRef, etc.). By walking the AST once to build a TypeMap (node_id → ExecType), we can lower shadow tree nodes that carry these types as speculative execution tags. The interpreter then runs fast-path nodes (Int32Add, DirectStringConcat, TypedArrayPush) and falls back to generic nodes on type guard failure.

**Performance target:** 3–6× faster than untyped shadow tree, 8–15× faster than QuickJS on fully-typed TypeScript.

## Architecture

### 1. ExecType Enum

Distilled execution types — not TS types, but VM execution categories:

```
Unknown           // No annotation, or any
Int32             // number (speculative integer)
Float64           // number (known float)
String            // string
Boolean           // boolean
Symbol            // symbol
BigInt            // bigint
Object(ShapeId)   // class/interface name → pre-resolved shape
Array(Box<ExecType>)
Tuple(Vec<ExecType>)
Union(Vec<ExecType>)
Literal(JSValue)  // literal type: "foo" | 42 | true
Void              // undefined
Function(Vec<ExecType>, Box<ExecType>)  // params, return
```

### 2. TypeMap

Side table mapping SWC node Span/NodeId to ExecType:

```rust
struct TypeMap {
    types: FxHashMap<NodeId, ExecType>,
}

impl TypeMap {
    fn from_swc_ast(module: &Module) -> Self {
        let mut map = TypeMap::default();
        let mut extractor = TypeExtractor { map: &mut map };
        module.visit_with(&mut extractor);
        map
    }
}
```

### 3. Type Extraction Rules

| SWC AST Node | Type Location | Example |
|--------------|---------------|---------|
| VarDecl / VarDeclarator | name: Pat::Ident { type_ann } | `let x: number` |
| FnDecl / ArrowExpr | return_type: Option<TsTypeAnn> | `function f(): string` |
| Param | pat: Pat::Ident { type_ann } | `(a: number)` |
| ClassProp | type_ann: Option<TsTypeAnn> | `x: number` |
| TsAsExpr / TsTypeAssertion | type_ann: TsType | `x as number` |
| TsConstAssertion | expr with const context | `const x = [1,2] as const` |

### 4. Shadow Tree Nodes with Type Guards

Every shadow tree node carries `expected_type: ExecType`. At execution, the node validates the actual value against the expectation, then either runs the fast path or deoptimizes.

Key nodes:
- `TypedAdd` — with AddState (Uninitialized, Int32Fast, DoubleFast, StringConcat, Generic)
- `TypedPropRead` — direct offset into pre-resolved Shape
- `TypedCall` — typed parameter hints at entry
- `TypedArrayOp` — packed storage (Int32, Float64, String, Object arrays)
- `TypedUnion` — variant dispatch by runtime tag
- `TypedFunctionEntry` — entry guard with per-parameter type validation

### 5. Type Guard Execution

```rust
// Example: TypedAdd with number hint
match state.get() {
    AddState::Uninitialized => {
        // First execution: validate against hint, pick state
        let new_state = match hint {
            ExecType::Int32 if a.is_int32() && b.is_int32() => AddState::Int32Fast,
            ExecType::Float64 | ExecType::Int32 if a.is_number() && b.is_number() => AddState::DoubleFast,
            ExecType::String if a.is_string() && b.is_string() => AddState::StringConcat,
            _ => AddState::Generic,
        };
        state.set(new_state);
        self.eval(stack, frames);
    }
    AddState::Int32Fast => {
        if likely(a.is_int32() && b.is_int32()) {
            // Fast path: no dispatch, no coercion
            let ai = a.as_int32();
            let bi = b.as_int32();
            match ai.checked_add(bi) {
                Some(res) => stack.push(JSValue::int32(res)),
                None => {
                    // Overflow: deopt to double
                    state.set(AddState::DoubleFast);
                    stack.push(JSValue::double(ai as f64 + bi as f64));
                }
            }
        } else {
            // Type guard failed — TS annotation was wrong
            state.set(AddState::Generic);
            stack.push(generic_add(a, b));
        }
    }
    // ...
}
```

### 6. Class-Aware Object Model

When TS gives a class type, the shape is known at parse time:

```rust
struct Shape {
    id: ShapeId,
    property_names: Vec<Atom>,
    offsets: Vec<usize>,
    property_types: Vec<ExecType>,  // expected type per slot
}

ShadowNode::TypedPropWrite {
    obj: ThisNode,
    shape_id: 42,           // Point's shape
    offset: 0,              // x slot
    expected_type: ExecType::Int32,
    value: ShadowNode::LocalRead(0),
}
```

Execution: No property lookup, no hash map. ~6 CPU instructions per property assignment.

### 7. Array Specialization

`Array<T>` becomes packed storage:

```rust
enum ArrayStorage {
    Generic(Vec<JSValue>),           // mixed types
    Int32(Vec<i32>),                  // 4× memory reduction
    Float64(Vec<f64>),
    String(Vec<Atom>),
    Object(Vec<NonNull<JSObject>>),
}
```

### 8. Memory Layout

```rust
struct TypedAddNode<'a> {
    tag: NodeTag,              // 1 byte
    hint: ExecType,            // 1 byte (packed enum)
    state: Cell<u8>,            // 1 byte (AddState packed)
    _pad: u8,                  // alignment
    left: &'a ShadowNode<'a>,  // 8 bytes
    right: &'a ShadowNode<'a>, // 8 bytes
} // 24 bytes total
```

Type hints are small enum tags (1 byte), not full AST references.

## Performance Impact

| Scenario | Untyped Shadow Tree | TS-Aware Shadow Tree | Speedup |
|----------|--------------------|-----------------------|---------|
| Class property access | Generic shape walk | Direct offset + guard | 3–5× |
| number arithmetic | Generic add w/ tag checks | Int32 fast path | 5–8× |
| Array<number>.push | Generic JSValue array | Packed Vec<i32> | 2× speed, 4× memory |
| string concatenation | Generic ToString + concat | Direct string concat | 3× |
| Typed function params | Generic coercion | Direct slot pass-through | 2–3× |
| typeof narrowing | typeof every use | Type-specialized branch | 2× |

**Overall:** 3–6× faster than untyped shadow tree on fully-typed TS.

## Honest Limits

TS types are not runtime guarantees. Guards handle:

| Lie | Example | Guard Behavior |
|-----|---------|----------------|
| any cast | `x as number` | Guard fails, falls back to generic |
| @ts-ignore | `let x: number = "foo"` | Guard fails, deoptimizes |
| Dynamic import | `import("evil.js")` | Module runs in Dynamic mode |
| eval | `eval("x = 'string'")` | Enclosing function marked Dynamic |
| External JS | Untyped library | Parameters treated as Unknown |

Mitigation: The VM is speculative. Type guards are cheap enough that lying costs only first execution + deopt. Generic fallback is always correct.

## Implementation Sketch

```rust
// Phase 1: SWC parse
let module = parse_with_swc(source)?;

// Phase 2: Type extraction pass
let type_map = TypeMap::from_swc_ast(&module);

// Phase 3: Shadow tree lowering with types
let shadow = ShadowTreeBuilder::new(&type_map)
    .with_class_shapes(true)
    .with_array_specialization(true)
    .with_union_dispatch(true)
    .build(&module);

// Phase 4: Execute
let mut vm = Vm::new();
vm.run(&shadow);
```

## Phases

- **Phase A:** Extract ExecType enum and TypeMap struct from SWC AST
- **Phase B:** Add typed shadow nodes (TypedAdd, TypedPropRead, TypedCall, TypedArrayOp)
- **Phase C:** Implement type guard execution with state machine (AddState)
- **Phase D:** Class shapes: parse-time Shape creation from ClassDecl
- **Phase E:** Array specialization: ArrayStorage enum and promotion logic
- **Phase F:** Union types and typeof narrowing blocks
- **Phase G:** Function signature specialization with entry guards

## Targets

- **Suite:** performance
- **Batch:** tbd
- **Blocked by:** Task 264 (typed HIR implementation)
- **Exit criteria:** Benchmark shows 3–6× improvement on typed TS workload

## Verification

- `cargo test` passes
- New tests in `tests/typescript/` for type-extraction scenarios
- Benchmark suite compares typed vs untyped shadow tree execution time
