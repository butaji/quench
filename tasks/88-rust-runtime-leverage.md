# Task 88 — Leverage Rust for runtime execution model

## Goal

Apply Rust's type system, ownership model, and ecosystem crates to make the interpreter safer, faster, and easier to extend. This task captures the Rust-specific decisions that support Task 85 (trampoline interpreter) and the future performance roadmap.

## Decisions to enforce

1. **Remove `Rc<RefCell<Object>>` from the hot path**
   - Store objects in a slot-indexed arena (`Vec<Object>` or `SlotMap`).
   - Reference objects by `ObjectId(u32)`.
   - Pass `&mut Context` through evaluation; mutate objects through `&mut` arena access.
   - Rationale: compile-time aliasing guarantees, no runtime borrow checks, predictable cache layout.

2. **Model the JS call stack explicitly**
   - Use `Vec<CallFrame>` inside a trampoline loop (Task 85).
   - Enforce `MAX_JS_STACK` as a simple vector-length check.
   - Rationale: eliminates native stack overflow, enables `try/catch`, generators, and async later.

3. **Keep `Value` as a direct enum**
   - Dispatch through `match`, not `Box<dyn Trait>` or vtables.
   - Defer NaN-boxing until after shapes and object model are stable.
   - Rationale: zero-cost dispatch, simplest path to correctness.

4. **Adopt Rust ecosystem crates at the right time**
   - `lasso` for identifier/property string interning (adopt with shapes).
   - `bumpalo` for AST/HIR scratch allocation.
   - `slotmap` for object storage if generational IDs are needed.
   - `rayon` for parallel conformance runner (fresh isolate per test).
   - `mimalloc` or `tikv-jemallocator` as global allocator once benchmarking starts.

5. **Concurrency model for isolates**
   - One JS isolate per thread, no shared mutable JS state.
   - Host ↔ runtime communication through channels.
   - Conformance runner spawns tests in isolated threads/contexts.

6. **Controlled use of `unsafe`**
   - Allow only for NaN-boxed `Value` representation and tightly encapsulated raw-pointer helpers.
   - Miri-test any `unsafe` block.
   - Never use `unsafe` to bypass borrow-checker discomfort.

## Acceptance criteria

- [ ] Task 85 trampoline interpreter uses `Vec<CallFrame>` and `&mut Context` with no `Rc<RefCell>` in the eval loop.
- [ ] Object storage is slot-indexed or uses `SlotMap`; `ObjectId` is a small copyable type.
- [ ] Conformance runner can run tests in isolated threads without a single failure killing the suite.
- [ ] Global allocator switch is measured with `cargo bench` / Criterion before/after.
- [ ] `unsafe` usage is documented and Miri-tested.

## Related

- Task 82 (whole-suite conformance)
- Task 85 (trampoline interpreter)
- `docs/architecture.md` Rust-specific leverage section
- `docs/research-findings.md` Maximizing Rust as a runtime section
