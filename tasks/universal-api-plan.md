# Universal JavaScript Engine API Plan

This plan defines the lowest-common-denominator embedding API for Quench,
based on the handle model shared by V8, QuickJS, JavaScriptCore,
SpiderMonkey, and Hermes. It is an additive embedding layer over the existing
OXC + wakler runtime architecture; it does not change the Test262 runner.

The Test262 digest remains the only conformance and progress source of truth.
Each implementation milestone requires focused Rust unit tests first, then
the affected Test262 stage, with no changes under `tests/test262`.

## API boundary

The public C ABI uses opaque handles:

```c
typedef struct js_rt  js_rt;
typedef struct js_ctx js_ctx;
typedef struct js_val js_val;
```

The native Rust API may use `Runtime`, `Context`, and `Value` directly. C
handles must be runtime-owned, context-checked, and invalidated on free; no
Rust layout or `Rc`/`RefCell` detail is exposed.

## Milestones

### U0 — ownership and error contract

- Define `Runtime` ownership, context lifetime, thread affinity, and handle
  validity rules.
- Define whether returned values are borrowed or rooted handles.
- Add `js_exception` get-and-clear semantics and an explicit `js_throw` path.
- Add ABI compile tests and lifecycle tests; no production API until those
  tests fail for the missing behavior.

### U1 — lifecycle and evaluation

Implement:

```c
js_rt*  js_rt_new(size_t max_heap, size_t max_stack);
void    js_rt_free(js_rt*);
js_ctx* js_ctx_new(js_rt*);
void    js_ctx_free(js_ctx*);
js_val* js_eval(js_ctx*, const char* source, const char* filename, int is_module);
js_val* js_global(js_ctx*);
```

Map `js_eval` to the existing context evaluation boundary, preserving reset,
microtask, exception, and module semantics.

### U2 — property and primitive construction

Implement `get`, `get_idx`, `set`, `set_idx`, and constructors for undefined,
null, boolean, number, string, object, array, and ArrayBuffer. All property
operations route through the canonical runtime operations rather than adding
an API-specific storage path.

### U3 — type checks and conversion

Implement the mandatory type predicates plus `to_bool`, `to_number`,
`to_string`, and ArrayBuffer extraction. Conversion errors use the normal
pending-exception contract and never panic.

### U4 — functions and calls

Expose native function registration and:

```c
js_val* js_call(js_ctx*, js_val* fn, js_val* this_, js_val** argv, int argc);
```

Verify strict `this` behavior, argument ownership, thrown exceptions, and
native callback userdata with Rust tests before wiring the ABI.

### U5 — promises and limits

Expose promise creation and resolve/reject. Add runtime GC and heap/stack
limit hooks only after the collector/allocator has a real enforcement point;
until then these functions must not be exported as false success APIs.

## Compatibility mapping

| Universal concept | Quench implementation target |
| --- | --- |
| Runtime / Isolate | runtime-owned allocator and intrinsic state |
| Context / Global | `Context` and its global environment |
| Value handle | rooted opaque wrapper around `Value` |
| Eval | `Context::eval` / module evaluation |
| Get / Set | canonical object property operations |
| Call | existing `call_value_with_this` path |
| Exception | pending `JsError` / thrown `Value` boundary |
| Promise | existing promise builtin and job queue |
| GC / limits | explicit allocator/runtime milestone, not a stub |

## Acceptance gates

For every milestone:

1. Add a failing unit test for the exact new contract.
2. Implement the smallest API surface that makes it pass.
3. Run the affected unit suite, formatting, and clippy with zero warnings.
4. Run the relevant Test262 stage and record its digest in the progress log.
5. Commit and push the milestone independently.

The API work must not regress the 100% Test262 target, increase Rust core
duplication, or introduce a second spec-operation implementation.
