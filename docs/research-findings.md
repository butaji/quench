> **Consolidated online research used to validate Quench's architecture and pick the highest-impact improvements.**

# Research Findings

For the actionable consequence of this research, see **`docs/minimum-custom-code-strategy.md`**.

## Executive summary

The project is architecturally sound: a dedicated Rust interpreter crate, SWC-based parsing, a custom HIR, and conformance harnesses are exactly how modern embeddable JS engines are built. The research confirms three priorities:

1. **Stability first** — replace recursive evaluation with an explicit call stack (trampoline).
2. **Conformance-driven correctness** — load real test262 harness files and fix top failure buckets with regression tests.
3. **Performance later** — NaN-boxing, shapes, and ICs are the right optimizations, but only after correctness is solid.

## Biggest wins (high impact / reasonable effort)

| Win | Effort | Impact | Source |
|-----|--------|--------|--------|
| Trampoline interpreter | Medium | Eliminates stack overflow; enables whole-suite runs | QuickJS, Boa, V8 Ignition, trampoline literature |
| Load test262 harness files from submodule | Low | Removes false failures; trustworthy test262 numbers | test262-harness, SpiderMonkey `jstests.py`, Jint harness |
| Skip "No baseline found" as skip | Trivial | Cleans TypeScript report noise | Own harness data |
| Thread-local depth counter + `MAX_JS_STACK` | Trivial/Low | Stops false stack-overflow failures | QuickJS stack-limit discussions |
| Object shapes + inline caches | Medium/High | Biggest JS-specific speedup | V8/Boa/JSC shapes & ICs articles |
| NaN-boxed `Value` | Medium | Faster value moves, less allocation | Standard engine practice |
| String interning (`lasso`) | Low/Medium | O(1) property/identifier comparisons | Boa `boa_interner`, lasso docs |

## Architecture patterns validated

### 1. Explicit stack / trampoline is the canonical way to avoid native stack overflow

- **QuickJS** uses an explicit JS stack + native C stack for locals, with a configurable max stack size.
- **Boa** compiles AST to an explicit VM stack (not the native Rust stack).
- **V8 Ignition** is a register-based interpreter with explicit frames.
- **Academic stackless designs** use CPS or a trampoline loop with a heap-allocated continuation stack.

**Decision:** Implement Task 85 (trampoline interpreter) before any large performance work. A `Vec<CallFrame>` + loop decouples JS recursion from Rust recursion and makes `try/catch`, generators, and `async/await` straightforward later.

### 2. HIR / explicit-stack interpreter is the correctness bridge

- A pure AST tree-walker is the fastest path to bootstrapping correctness.
- A typed HIR with an explicit `Vec<CallFrame>` loop gives us the same stack-safety and extension points as register-machine engines without adding a separate bytecode layer.

**Decision:** Keep the HIR high-level and serializable so it can feed future optimizations without a rewrite.

### 3. Object shapes (hidden classes) + inline caches are non-optional for fast property access

- Every major engine uses shapes: V8 calls them Maps, JSC Structures, SpiderMonkey Shapes, Boa Shapes.
- Shapes separate property keys/attributes from values, shared across objects with the same creation order.
- Inline caches store `(shape_id, offset)` at property-access sites; monomorphic hits become a shape check + array load.

**Decision:** Implement shapes + ICs after the value model is stable. This is the single biggest JS-specific performance win.

### 4. Value representation can stay simple until performance matters

- **Boa** uses `JsValue` enum + `Gc<T>`.
- **QuickJS** uses tagged unions.
- Both start correct and optimize later.

**Decision:** Keep the current `Value` enum while adding features. Move to NaN-boxing only after shapes and a stable object model are in place.

### 5. String interning is standard for identifiers and property names

- **Boa** has `boa_interner`.
- **lasso** is the recommended Rust interner: O(1) intern/resolve, multi-threaded `ThreadedRodeo`, `RodeoReader`/`RodeoResolver` for read-heavy phases.
- Property access should compare `Atom(u32)`, not string bytes.

**Decision:** Use `lasso` for identifiers and property names when shapes are introduced.

## Conformance harness best practices

- **Load harness files from `tests/test262/harness/`**: `assert.js`, `sta.js`, `compareArray.js`, `propertyHelper.js`, etc. should be parsed and executed by the engine before each test. Stubbing them causes false failures.
- **Execute every test262 test; do not pre-skip.** Unsupported features should fail and be bucketed, not skipped. This is the highest-impact harness change because it reveals the true pass rate and the largest failure buckets.
- **Use fresh contexts per test** to avoid state leakage.
- **Run cases in isolated threads** so a stack overflow in one case does not kill the runner.
- **Report by feature/category** so progress is measurable.
- **Reference runners** (TypeScript's own `hereby`) are useful for baseline validation but not for daily development.

## Technology choices

| Area | Current | Validated alternative | Recommendation |
|------|---------|----------------------|----------------|
| Parser | swc | oxc-parser (~3× faster, passes test262 stage 4) | **Stay on swc.** Already integrated; switching adds migration risk with no correctness payoff before 100% conformance. |
| String interning | none | `lasso` | **Adopt `lasso` now** for identifiers and property names. |
| Regex | none | `regress`, `regex` | **Adopt `regex` crate** with a JS-syntax adapter when regex support is needed. |
| BigInt | none | `num-bigint` | **Adopt `num-bigint`** when BigInt support is needed. |
| GC | `Rc<RefCell<Object>>` | Boa's `boa_gc`, Immix | **Keep `Rc` for now.** Move to tracing GC only if cycles become a measurable problem. |
| Global allocator | default | `mimalloc`, `tikv-jemallocator` | **Add when benchmarking starts**; not a correctness prerequisite. |

## Maximizing Rust as a runtime

Rust is not just the implementation language — its type system and runtime model should be used as guardrails and accelerators:

### 1. Compile-time VM invariants
- Use ownership and lifetimes so that an `ObjectId` cannot outlive the `Context` that owns it.
- Model the JS call stack as `Vec<CallFrame>` and pass `&mut Context` through the interpreter. The borrow checker then prevents use-after-free and aliasing bugs at compile time.
- Avoid interior mutability (`RefCell`) in the hot loop; prefer `&mut` arena access and index-based mutation.

### 2. Zero-cost data structures on the hot path
- `Value` as an enum with small variants lets the interpreter loop dispatch through a single `match` with no vtable cost.
- Slot-indexed object storage (`Vec<Object>` or `SlotMap`) replaces `Rc<RefCell<Object>>`: object references become `ObjectId(u32)`, access is a bounds-checked array index, and allocation/deallocation is controlled by the runtime.
- Object shapes separate shared layout from per-object values, so shape transitions and inline-cache checks are cheap integer/array operations.

### 3. Controlled memory and allocation
- Use `bumpalo` for short-lived AST/HIR allocations.
- Use `lasso` to intern identifiers and property names; this turns the most common comparison (property lookup) into a `u32` equality check.
- Delay a tracing GC. Start with reference counting and move to a generational/Immix collector only when cyclic object graphs are measurable.
- Switch the global allocator to `mimalloc` or `tikv-jemallocator`; JS workloads allocate heavily and this is a one-line win.

### 4. Fearless concurrency for isolates and harness
- Run each JS isolate in its own thread; share nothing mutable between isolates.
- Use channels for host ↔ runtime communication.
- Parallelize the conformance runner with `rayon`, running each test in a fresh isolate. A single stack overflow then fails one test, not the whole suite.

### 5. When to use `unsafe`
- Keep `unsafe` isolated to value representation (NaN boxing) and tight raw-pointer sequences. Encapsulate it behind safe APIs and test with Miri.
- Do not use `unsafe` to bypass the borrow checker for convenience; redesign the boundary instead.

### 6. Low-effort toolchain wins
- Build with `lto = "thin"` and, later, profile-guided optimization.
- Add `cargo bench` / Criterion for interpreter micro-benchmarks before any optimization.
- Use `thiserror` for structured runtime errors and `clippy` pedantic lints to keep the codebase maintainable.

## What to avoid

- **Premature optimization.** Build correctness first; performance work comes only after the conformance suites pass.
- **Custom parser/lexer.** swc and oxc already solve this; never hand-roll one.
- **`RefCell` in hot paths.** Shapes and slot-indexed environments should use `&mut` arena access.
- **String comparisons for property names.** Always use atoms once interning is in place.
- **Box<dyn Trait> / vtables in the interpreter hot loop.** Use direct `match` dispatch.

## Risks

- **Trampoline rewrite is intrusive.** It touches every eval path. It must be done in small, tested steps with regression tests.
- **test262 coverage is huge.** 53k+ tests mean progress is measured in percentage points, not absolute passes. Focus on feature buckets.
- **TypeScript baselines are not always runnable.** Some cases are type-check only; skip them cleanly.

## Sources

- QuickJS internals: <https://bellard.org/quickjs/quickjs.html>
- QuickJS stack overflow issue #55: <https://github.com/bellard/quickjs/issues/55>
- Boa engine & shapes release: <https://boajs.dev/blog/2023/07/08/boa-release-17>
- Boa on GitHub: <https://github.com/boa-dev/boa>
- Shapes & Inline Caches (Mathias Bynens): <https://mathiasbynens.be/notes/shapes-ics>
- Trampoline / stackless interpreter paper: <http://soft.vub.ac.be/Publications/2017/vub-soft-tr-17-10.pdf>
- test262 harness: <https://github.com/bterlson/test262-harness>
- test262-harness-dotnet (Jint): <https://github.com/lahma/test262-harness-dotnet>
- SpiderMonkey test docs: <https://firefox-source-docs.mozilla.org/js/test.html>
- Oxc parser benchmarks: <https://github.com/oxc-project/bench-javascript-parser-written-in-rust>
- lasso interner: <https://github.com/Kixiron/lasso>
- String interners in Rust: <https://dev.to/cad97/string-interners-in-rust-797>
- Safe tracing GC designs in Rust: <http://manishearth.github.io/blog/2021/04/05/a-tour-of-safe-tracing-gc-designs-in-rust/>

## Implication for priorities

The ranked list in `docs/conformance.md` and Tasks 82/85/88 remains correct:

1. Trampoline interpreter (Task 85) — biggest stability win.
2. Load real test262 harness files — biggest conformance-report win.
3. Fix top failure buckets with regression tests — biggest correctness win.
4. Apply Rust runtime model (Task 88) — slot-indexed objects, isolate threads, allocator/interning.
5. Shapes + ICs + NaN-boxing — biggest performance win, but only after 1–4.
