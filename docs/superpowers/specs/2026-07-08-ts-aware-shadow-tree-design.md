# TS-Aware Shadow Tree Interpreter

## Goal
Harvest TypeScript annotations from SWC's AST during shadow-tree lowering and attach distilled execution types (`ExecType`) to nodes. The interpreter runs type-guarded fast paths and deoptimizes locally when runtime reality disagrees with the annotation.

## Scope
First milestone:
1. Build a `TypeMap` from SWC AST annotations (`number`, `string`, `boolean`, literal types, class type refs).
2. Add typed shadow nodes: `TypedAdd`, `TypedPropRead`.
3. Execute with type guards and in-place deoptimization.
4. Add regression tests.

Defer to later: array storage specialization, union dispatch, typed function entry guards, `typeof` narrowing.

## ExecType
```rust
pub enum ExecType {
    Unknown,
    Int32,
    Float64,
    String,
    Boolean,
    Symbol,
    BigInt,
    Object(ShapeId),
    Literal(JSValue),
    Void,
}
```

## TypeMap extraction
- Walk the SWC script AST.
- For every `Pat::Ident` with `type_ann`, map the binding's `Span`/`NodeId` to an `ExecType`.
- For `TsTypeRef` with a class name, look up the shape in `ShapeInterner` (create it if the class is in scope).
- For literal types, encode as `ExecType::Literal(JSValue)`.
- Store `TypeMap` keyed by a stable identifier derived from the SWC node.

## Typed nodes
```rust
pub enum ShadowNode<'a> {
    // ... existing variants ...
    TypedAdd {
        left: &'a ShadowNode<'a>,
        right: &'a ShadowNode<'a>,
        hint: ExecType,
        state: Cell<AddState>,
    },
    TypedPropRead {
        obj: &'a ShadowNode<'a>,
        prop: Symbol,
        obj_hint: ExecType,
        cache: Cell<PropCache>,
    },
}
```

## Execution
- `TypedAdd`: first execution validates operands against `hint`, picks `Int32Fast`/`DoubleFast`/`StringConcat`/`Generic`, then runs the fast path with a guard. On guard failure, degrade to `Generic`.
- `TypedPropRead`: if `obj_hint` is `Object(shape_id)`, verify the object's shape matches and read directly from the cached offset; otherwise run generic property lookup.

## Verification
- Unit tests in `shadow.rs`:
  - `test_typed_add_number_hint`
  - `test_typed_add_string_hint`
  - `test_typed_add_deopt_when_lying`
  - `test_typed_prop_read_class_hint`
- `cargo test --workspace` green.
