# ADR 0003: Stackless VM execution + isolate-bounded heap

- Status: Proposed
- Date: 2026-08-21
- Supersedes / complements: ADR 0001, ADR 0002; `docs/architecture.md`; `docs/data-first-minimal-runtime.md`; `docs/compatibility-contract.md`
- Cluster: rules 1–10 (stackless VM); 31–40 (shape/slot objects, Phase A contract); 81–90 (cache line / SoA / AoS); 91–95 (fixed-width IDs)

## Decision

Adopt **`execution = transition(state, instruction)`**, **`stack = data`**, **`memory = budgeted data`**.

Two coupled invariants:

1. **No recursive host frames for guest JS calls, `eval`, generators, `await`, construct, proxy traps, or exception propagation.** The interpreter is one isolate-owned loop over explicit activations and control frames. Native Rust stack depth stays roughly constant regardless of guest call depth.
2. **Every JS-owned allocation is charged against an isolate-level memory budget** with hard caps and per-class limits. A tracing GC owns the heap; any arena reset for transient objects remains an explicit, benchmarked lifetime-domain experiment rather than a request-wide semantic assumption. OOM is recoverable by killing/resetting the isolate, never the process.

A tail-call trampoline, a TLS depth counter, or a recursive `run_until_this_frame_returns` used as the ordinary call path is **not** a stackless VM. `functions_arguments_execution.rs::execute_frames` is that trampoline today and must be deleted, not extended.

## Context — current control graph (source)

Observed 2026-08-21. This is the implementation surface any migration must replace.

### Call entry still recurses on the host stack

```text
functions_arguments.rs::execute_target          // TLS CallDepthGuard, MAX_CALL_DEPTH=40_000
  ├─ Builtin / HostCapability
  ├─ Function -> execute_in_function_realm
  │                └─ with_realm? -> functions_arguments_execution.rs::execute
  ├─ BoundFunction -> execute_bound / execute_bound_function
  └─ Proxy -> proxy.rs::proxy_apply -> execute_target(target) | call_trap
```

`execute` (`functions_arguments_execution.rs:59`):

- class constructor without `new` → `TypeError`
- `FunctionKind::Generator` → `generator::create` (no interpreter frame)
- `function.is_async` → `execute_frame_value` then `promise::from_async_completion`
- else → `execute_frames(CallFrame)`

`CallFrame` is `{ function, receiver, arguments }`. It is **not** a PC, register window, environment, return destination, or handler stack.

`execute_frames` (`:116`) loops only on `Completion::TailCall`. Every other completion returns. Nested `Op::Call` never reaches this loop.

### Interpreter still walks one function body then returns

```text
execute_frame_completion
  TLS: private_environment::Guard, super_scope::Guard, with_scope::FunctionGuard
  build_registers(function, receiver, arguments)
  vm_execution.rs::execute_frame_completion
    TLS: ContextGuard, GlobalObjectGuard, locals::EnvironmentGuard
    vm_runtime.rs::run_ops_completion
      run_ops_completion_step_from          // for (index, op) in ops[start..]
        vm_dispatch.rs::run_op
```

`run_ops_completion_step_from` (`vm_runtime.rs:38`) is a linear scan. A non-`Normal` `Completion` exits the scan. Nested bodies (`Try`, `Loop`, `Branch`, `With`, `Eval`, …) re-enter `execute_completion_in_place` on the host stack.

### Call / construct / await / tail are host-stack effects

| Site | Current behavior |
|---|---|
| `vm_dispatch.rs::hot_call` / `run_call` | `vm_ops::execute_call` → `invoke_with_receiver` → `functions::execute_target` |
| `run_method_or_construct` | `methods::execute` / `super_scope::execute_call` / `execute_constructor` / `construct::execute` |
| `methods.rs::execute_callee` | `Function` → `execute_target`; `BoundFunction` → `execute_bound`; builtins stay in host |
| `vm_ops.rs::prepare_tail_call` | builds `completion::TailCallRequest { callee, receiver: Undefined, arguments }` |
| `execute_frames` + `resolve_tail_target` | rewrites `CallFrame` **or** calls builtin/proxy/async/generator synchronously |
| `run_await_completion` | `vm_ops::execute_await`: `promise_resolve`, drain microtasks, else `VmError::Suspended` |
| `construct.rs::construct_function` | `functions::execute_construct` → `vm::execute_in_environment` (new host frame) |
| `super_scope.rs::execute_constructor` | `construct::construct_super` then instance-field `execute_target` |
| `proxy.rs::proxy_apply` / `proxy_construct` | trap via `call_trap` or recurse onto `execute_target` / `construct_value_with_new_target`; construct trap must return an object |
| `reflect.rs::execute_eval` | non-`%Eval%` → `execute_target`; direct/indirect eval compiles and `execute_in_environment` / `execute_indirect_eval` |
| Array/Map/Set/iterator builtins | `execute_target(callback, …)` per element |

### Two machines already exist; ordinary calls use neither

`machine.rs` already has the compact ABI from `docs/architecture.md`:

- `Machine { store, code: CodeId, pc, registers: RegisterWindow, environment: EnvironmentRef, completion: Completion, frames: FrameStack }`
- `Frame` = `Try` / `Iterator` / `Await` / `Delegate` / `Branch` / `Private` with mandatory phases (`TryPhase`, `IteratorPhase`, …)
- `FrameStack { base, count, frames, limit }` — **default `limit = 64`**, control-frame capacity, not JS call depth
- `Machine::step(input, execute)` still takes a host closure; it is not the isolate loop
- `FunctionCode` / `CodeStore` already flatten nested bodies to `CodeRange`

Generators own a `Machine` (`generator.rs::create`, `generator_machine.rs`). Ordinary `Call` / `Construct` / `eval` do **not**. Async non-generators run `execute_frame_value` to the first pending `Await` and convert `VmError::Suspended` in `from_async_completion` by leaving a promise pending **without** parking an activation (resume exists only for `GeneratorData` via `PromiseContinuation::AsyncGenerator`).

`functions_receiver.rs::execute_target_with_receiver` duplicates `execute_frame_completion` and reads `this` back from environment slot `captures + params + 1`.

### Why a recursive shim is forbidden

`execute_frames` + `CallDepthGuard` is the current “pragmatic” shape. It cannot:

- keep host stack constant under `f(){ f() }` or `richards` scheduler recursion
- throw `RangeError("Maximum call stack size exceeded")` on the default stack (the 40 000 counter is reached only if the host stack survives)
- suspend an ordinary async function and resume it
- walk `try`/`finally` across a call without the host stack still holding the `exceptions::execute` frame
- let a proxy trap or `Array.prototype.map` callback nest without another Rust frame

Raising the dedicated thread stack, wrapping `execute_target` in `run_until_frame_returns`, or trampolining only `TailCall` are rejected.

## Decision detail — one isolate machine

```text
Isolate
  ├── heap, interners, modules, microtasks          // budgeted; see heap section
  ├── machine: Machine                              // the only interpreter
  ├── activations: Vec<Activation>                  // JS call stack; bounded
  └── continuations: table<ContinuationId, Continuation>
```

`Machine` stays the type in `machine.rs`. It is the live execution cursor, not a per-function object. `GeneratorData.machine` becomes a parked `ContinuationId` plus the generator’s owned register/environment payload; while running, that payload is installed on `Isolate.machine`.

### Activation (JS call / construct / eval)

Compact, no leftover `arguments: Vec<Value>` after `build_registers`.

```text
Activation {
  function: Rc<FunctionValue>,          // until FunctionValue → CodeId + heap
  code: CodeId,                         // function.code.code_id()
  range: CodeRange,                     // function.code.range
  pc: u32,                              // next op; same unit as Machine.pc
  registers_base: u32,                  // into a shared register file, or owned RegisterWindow
  registers_count: u16,                 // pre-sized: ops.len() + params + captures + 8, min 32
  environment: EnvironmentRef,          // plus parked Rc<Environment> until heap migration
  realm: RealmId,
  kind: ActivationKind,
  dest: ReturnDest,
  new_target: Option<Value>,            // Construct / CallSuperConstructor
  this_slot: u16,                       // captures + params + 1 (same as execute_construct)
  new_target_slot: u16,                 // this_slot + 1
  frame_base: u16,                      // first control Frame belonging to this activation
  home: Option<Value>,                  // replaces super_scope::Guard
  private_environment: PrivateEnvironment,
  with_captures: Vec<Value>,            // replaces with_scope::FunctionGuard
}

enum ActivationKind {
  Script,
  Call,                 // Op::Call / CallMethod / CallSuperMethod / Function.prototype.call
  Construct,            // Op::Construct / CallSuperConstructor / new
  Eval,                 // reflect::execute_eval direct / indirect
  Generator,            // generator::resume
  Async,                // function.is_async && kind != Generator
  Host,                 // builtin that requested a JS call (see HostCall)
}

enum ReturnDest {
  Register { slot: u16 },               // caller register (Op::Call.dst, CallMethod.dst, …)
  Construct { dst: u16 },               // apply construct_function / finish_derived_construct
  SuperConstruct { dst: u16, this_slot: u16 },
  Eval { dst: u16 },
  Promise { promise: Rc<PromiseData> }, // async function root
  Generator { generator: Rc<GeneratorData> },
  Host { resume: HostResume },          // builtin continuation
  Discard,                              // script / fire-and-forget job
}
```

Register sizing copies `build_registers` (`functions_arguments.rs:78-86`) and `execute_construct` this/new.target slots (`:114-119`). Sloppy `this` boxing stays `vm_runtime.rs::bare_call_receiver`. Class constructors without `new` still fail before push (`execute:68-71`).

`activations.len()` is the JS call depth. Push is checked first:

```text
if isolate.activations.len() >= MAX_FRAMES {
    return Transition::Throw(range_error("Maximum call stack size exceeded"));
}
```

`MAX_FRAMES` replaces `MAX_CALL_DEPTH`. Product default: **10_240** (Node-like; Node threw at depth 10405 on the recorded probe). Configurable per isolate. `CallDepthGuard` and `CALL_DEPTH` TLS are deleted only after every `execute_target` caller is a transition.

`machine::FrameStack` remains the **control** stack. Do not reuse its default limit of 64 as the JS call cap.

### Control `Frame` (already in `machine.rs`)

Keep the existing enum. Do not invent a second try/iterator/await walker.

| Variant | Phase contract (`docs/architecture.md`) | Current recursive owner |
|---|---|---|
| `Try` | `Body → Catch → Finally → Resume` | `exceptions.rs::execute` |
| `Iterator` | `Fetch → Bind → Body → Continue`, `Close` on abrupt | `loops.rs::execute_for_in/of`, `collections::iterator` |
| `Await` | `Evaluate → Pending → Fulfilled \| Rejected` | `vm_ops::execute_await`, `generator_machine::update_await_frame` |
| `Delegate` | `Open → Resume → Yield \| Complete` | `generator::execute_yield_star` |
| `Branch` | `Body → Resume` | `branch.rs`, `conditional.rs`, `switch.rs` |
| `Private` | `Body → Resume` | `private_environment::execute_scope` |

A control frame is pushed when the corresponding `Op` would today recurse into a nested `FunctionCode` body. The nested ops are a `CodeRange` in the same `CodeStore` (`FunctionCode::from_ops_many`). `Machine.pc` moves into that range; it does not call `execute_completion_in_place`.

### `Transition` — the only interpreter result

Replace `run_op`’s `Result<Option<Completion>, VmError>` as the **loop** contract. Intra-op arithmetic/property still use `Result` locally; they become `Throw` at the edge (`Completion::from_vm_error` already maps `Thrown` / `Suspended` / `NotCallable`).

```text
enum Transition {
  Continue,                                 // pc += 1
  Jump { pc: u32 },                         // Branch / Loop / Label / Switch
  Push { activation: Activation },          // Call / Construct / Eval / Generator resume
  Tail { activation: Activation },          // pop current, push next; depth unchanged
  Return { value: Value },                  // apply ReturnDest, pop activation
  Throw { value: Value },                   // walk handlers; see below
  Yield { value: Value },                   // park Continuation; return iterator result
  Suspend { promise: Rc<PromiseData> },     // park Continuation; schedule resume
  Host { builtin, receiver, arguments, dest, kind: HostKind },
}

enum HostKind { Call, Construct, Trap { name: &'static str, proxy: Value } }
```

`Completion` (`completion.rs`) remains the **semantic** algebra (`Normal`, `Return`, `TailCall`, `Throw`, `Break`, `Continue`, `Suspend`, `Yield`). The loop **derives** `Transition` from `(Op, Completion, ActivationKind)`. Packed storage is `identity::PackedCompletion { tag, flags, payload, aux }` once heap refs exist; until then `Machine.completion` stays the enum.

`Break` / `Continue` are not host errors. They search control frames for a matching `Iterator` / `Branch` label and `Jump`. Uncaught → `Throw` only at script boundary if the current `into_vm_error` path would have produced `VmError::Break`.

### Isolate loop

One function, e.g. `vm_runtime.rs::run_isolate(isolate) -> Result<Value, VmError>` (script/job result only).

```text
loop {
    let Some(activation) = isolate.activations.last() else { return machine.completion };
    let op = isolate.code(activation.range, machine.pc);
    match dispatch(isolate, op) {
        Continue            => machine.pc += 1,
        Jump { pc }         => machine.pc = pc,
        Push { next }       => { check_depth(); park_pc(); activations.push(next); install(next); }
        Tail { next }       => { check_depth_unchanged(); pop_install(next); }
        Return { value }    => apply_return(isolate, value)?,
        Throw { value }     => unwind_throw(isolate, value)?,
        Yield { value }     => return park_yield(isolate, value),
        Suspend { promise } => { park_await(isolate, promise); if activations.is_empty() { return pending } }
        Host { .. }         => run_host_or_push_js(isolate, …)?,
    }
}
```

`dispatch` is the existing `vm_dispatch.rs` table (`HOT_DISPATCH`, `run_simple_op`, `run_control_op`, `run_dispatch_op`) with call/control arms changed from “invoke and write `dst`” to “return `Transition`”. Hot arithmetic/property arms stay `Continue`.

### Return destinations

| Producer | `ReturnDest` | Apply |
|---|---|---|
| `Op::Call { dst, … }` | `Register { dst }` | write caller register; `pc` already past the call |
| `Op::CallMethod { dst, … }` | same; plus `propagate_updated_object` for `MapSet`/`SetAdd` | `methods.rs:24-31` |
| `Op::Call { … }` object-mutation | `Register` + `vm_ops::propagate_object_mutation` for `ObjectDefineProperty` | |
| `Op::Construct { dst, … }` | `Construct { dst }` | `construct_function` finish: object result else `this` else allocated receiver; derived uses `finish_derived_construct` |
| `Op::CallSuperConstructor { dst, … }` | `SuperConstruct { dst, this_slot }` | reject double-init (`super_scope.rs:193-196`); `initialize_instance_fields`; write `this_slot` and `dst` |
| `Op::Eval { dst, … }` | `Eval { dst }` | write `dst` |
| async root | `Promise { promise }` | `resolve_promise` / `reject_promise` (`from_async_completion`) |
| `generator::resume` | `Generator { generator }` | `complete_step` / iterator result object |
| builtin callback | `Host { resume }` | restore host kernel PC (index / accumulator) |

`Op::Return` on a `Construct` activation does **not** blindly return the register: it runs the same object/`this` selection as `construct_function:390-399`.

`Op::TailCall` (`vm_ops::prepare_tail_call` + `resolve_tail_target` / `flatten_bound_target`):

- flatten `BoundFunction` chains **before** push (same as `flatten_bound_target`)
- `Function` ordinary/method/arrow, not async, not generator → `Transition::Tail` (depth unchanged, reuse register file when counts allow)
- `Builtin` / host → `Host` then `Return` the value into the **caller’s** dest (the tail frame is already gone)
- `Proxy` → `Host { kind: Trap { "apply" } }` then dest
- async / generator → `Push` a new activation of that kind; the tail caller still pops (observable: the generator object / promise is the tail result)

### Exception walk

`Op::Throw` and `VmError::Thrown` become `Transition::Throw`. No `exceptions::execute` recursion.

```text
unwind_throw(isolate, value):
    while let Some(frame) = isolate.machine.frames.last() {
        match frame {
            Try { phase: Body, handler: Some(range), catch_slot, .. } => {
                bind_caught(value, catch_slot);  // exceptions.rs::bind_caught
                frame.phase = Catch;
                machine.pc = range.start;
                return;
            }
            Try { phase: Body | Catch, finalizer: Some(range), .. } => {
                frame.phase = Finally;
                park thrown as pending completion;
                machine.pc = range.start;
                return;
            }
            Try { phase: Finally, .. } => {
                // finalizer completed with Normal: rethrow parked
                pop frame; continue;
            }
            Iterator { .. } => { IteratorClose if required; pop; continue; }
            Await | Delegate | Branch | Private => { pop; continue; }
        }
    }
    apply_return_or_reject_current_activation(Throw(value));
    if activations remain { continue loop } else { isolate result = Throw }
```

Finally precedence matches `exceptions.rs`: an abrupt completion from the finalizer (`run_finalizer` / `finish_abrupt_finally`) replaces the parked completion. `dst` / `finally_dst` writes stay.

Uncaught throw at the isolate root is still `VmError::Thrown`. It never aborts the process.

### Await and generator continuations

One continuation type. `continuation.rs::SuspensionPoint` (`Yield` / `YieldStar`) is absorbed as parked `Frame::Delegate` / `pc`.

```text
Continuation {
  id: ContinuationId,
  activations: Vec<Activation>,         // from isolate root down to the suspensor
  frames: Vec<Frame>,
  registers: RegisterWindow,            // or shared-file slice copy
  environment: Rc<Environment>,
  completion: Completion,               // resume input
  promise: Option<Rc<PromiseData>>,     // Await
  generator: Option<Rc<GeneratorData>>,
}
```

**Await (`Op::Await { dst, src }`)** replaces `vm_ops::execute_await`:

1. `value = promise_resolve([registers[src]])` (same as today).
2. Already `Fulfilled` and not `module_bindings::fulfilled_await_defers()` → write `dst`, `Continue`.
3. `Rejected` → `Throw(reason)`.
4. `Pending` (or deferred fulfilled) → push `Frame::Await { phase: Pending, resume: CodeRange { start: pc+1, … } }`, park `Continuation`, `Transition::Suspend`. **Do not** `drain_microtasks_all` inside the op. The isolate job loop drains.

Resume (`promise.rs::process_async_continuation` generalized):

- load continuation onto `Isolate.machine` / `activations`
- write fulfillment into `Await` dest register
- rejection → `Transition::Throw` at the await site (so `try` around `await` works)
- `step` is the only entry (`Machine::step` becomes this install + loop, no host closure)

**Generators** (`generator.rs::{create,resume,next,return_,throw}`):

- `create` still allocates `GeneratorData` and runs parameter ops up to `Op::ParameterEnd` as a bounded activation that cannot nest user calls without pushing (parameters may still evaluate defaults — those are real calls)
- `resume` is `Push { kind: Generator, dest: Generator { … } }` with `Resume::Next/Return/Throw` as the input `Completion`
- `Yield` parks and returns the iterator result; it does not unwind activations above the generator root
- `YieldStar` is `Frame::Delegate`, not a second stepper (`run_yield_star_step` / `execute_yield_star` collapse into the isolate loop)
- `generator_machine.rs::{push_try_frame,push_iterator_frame,push_branch_frame,push_private_frame,update_await_frame}` become the **same** control-frame pushes ordinary code uses
- `executing` / `running` flags stay as generator re-entrancy guards (`TypeError: Generator is already executing`)

**Async functions** (`is_async && kind != Generator`) are `ActivationKind::Async` with `ReturnDest::Promise`. They share Await frames. `from_async_completion` is only the root dest applicator, not an execution mode. The current “run until suspend and drop the frame” path is a semantic hole and is removed.

### Construct, proxy, eval

**Construct** (`construct.rs::construct_with_new_target`):

- `Builtin` / `HostCapability` stay `Host` (no JS body)
- `Function` → `Push { kind: Construct, new_target, this_slot initialized or uninitialized if `is_derived_constructor` }`
- default derived constructor (`is_default_derived_constructor`) → `Push` of the super constructor with the same `new_target`, then `initialize_instance_fields` as a follow-up `Host` or small activation
- `BoundFunction` → flatten then same as target (`construct_bound_target`)
- `Proxy` → below

**Proxy** (`proxy.rs`):

- `apply`: if trap present → `Host { Trap { "apply" } }` calling the trap with `[target, thisArg, argsArray]`; if the trap is JS, that is a `Push`. After return, no extra invariant. If no trap → `Push`/`Host` on `proxy.target`.
- `construct`: trap present → `Push` trap `[target, argsArray, newTarget]`; **after the trap returns**, enforce “result is object” (`proxy_construct:309-313`). That check is a `HostResume` step, not something the trap activation knows. Missing trap → construct the target with `new_target`.
- Revoked proxy: `Throw` before push (`check_revoked`).
- Nested proxy targets flatten iteratively in the loop, not by Rust recursion (`proxy_apply:276`).

**Eval**:

- non-`%Eval%` callable: ordinary `Push` / `Tail` (`reflect.rs:291-303`)
- `%Eval%`: compile to `FunctionCode`, `Push { kind: Eval, dest: Eval { dst } }` with the eval environment (`evaluate_direct` / `evaluate`). Direct eval keeps the current lexical environment; indirect uses `execute_indirect_eval`’s global child. Eval is a real activation and counts toward `MAX_FRAMES`.

### Host kernels that call JS

Builtins (`arrays.rs`, `builtins_core.rs`, `collections/map.rs`, `set.rs`, iterator helpers, `conversion.rs` `toPrimitive`, class instance fields, `Function.prototype.call` / `apply`) currently call `execute_target` and therefore grow the host stack.

Stackless rule: a host kernel may **compute** without JS; the moment it must invoke a callable it returns `Transition::Host` / `Push` and saves `HostResume { kernel, index, acc, dest }`.

Until every kernel is converted, a **temporary** `isolate.enter(activation)` is allowed **only** for host kernels, and only if:

- it pushes onto the **same** `Isolate.activations` (so `MAX_FRAMES` still holds)
- it does not start a second `Machine`
- it is listed, counted, and removed by phase 5

`isolate.enter` is not the JS-to-JS path. Shipping it as the ordinary `Op::Call` implementation fails this ADR.

Thread-locals that encode “current frame” (`locals::CURRENT_ENVIRONMENT`, `super_scope::CURRENT`, `private_environment::CURRENT`, `with_scope::OBJECTS`, `vm_context::CURRENT_CONTEXT`, `CALL_DEPTH`, `loops::LIVE_FOR_OF`) move onto `Activation` / `Machine`. Guards remain only as install/restore around `run_isolate` for host code that has not been updated.

## Decision detail — heap budget and classes

Unchanged in intent, but these are **target invariants, not current behavior**. The current runtime has `HeapArena<T> = Vec<Option<T>>` with a manual free list, thread-local `ArrayBuffer` byte accounting, and no tracing collector or isolate-wide budget. `Rc` graphs can retain cycles; strings, object/array storage, code, continuations, microtasks, and host/external allocations are not yet charged by one owner. `ArrayBufferData::new` still has an expect/panic path, and the current thread-local counter is not an isolate boundary.

```text
Isolate {
    heap_limit: usize,
    heap: TracingHeap,
    machine: Machine,
    activations: Vec<Activation>,
    continuations: ContinuationTable,
    strings: Interner,
    modules: ModuleCache,
    microtasks: MicrotaskQueue,
    arena: ArenaRegion,
}
```

Target charged classes: arrays, `ArrayBuffer`, objects, strings, code, native/external, **activation register files**, **parked continuations**.

Target hard caps: array length `≤ 2^32-2`; string length `≤ 2^29-1`; arraybuffer `≤ 2^48`; object properties `≤ 2^16`; JSON nesting `≤ 2^12`; module depth `≤ 2^8`; microtask queue `≤ 2^16`; regex work configurable; **`activations.len() ≤ MAX_FRAMES`**.

Target behavior on `heap_limit` exhaustion: collect; if still over, `RangeError("Allocation failed")` at the isolate boundary. The current implementation does not yet provide this end-to-end behavior.

Target invariant: `ArrayBuffer` and strings cannot bypass `heap_limit` via raw `Vec<u8>`. Current accounting covers only a thread-local ArrayBuffer byte counter and does not cover all string/external allocations.

Until tracing GC and isolate ownership land, `Rc` cycle leaks and unaccounted classes remain open risks; the existing counter does not by itself make OOM recoverable.

## Decision detail — regions for transients

Bump `ArenaRegion` per request/job. Non-observable scratch (operand buffers, decoder scratch, reused register files from a free list) goes through the arena. Parked continuations and live activations are heap roots, not arena.

## Migration phases

Each phase is a semantic cutover for its listed symbols. No phase is “add a recursive helper and call it stackless.” The tree is **not** a stackless VM: ordinary JS calls still recurse on the host stack, and the live `Transition` type is a two-arm shim, not the isolate-loop algebra in “Decision detail”.

### Current Transition failure (observed 2026-08-21)

This is the live contract. It is a regression against implicit JS return, not progress toward stackless execution.

```text
// crates/quench-runtime/src/vm/activation.rs
enum Transition {
  Continue,                 // keep scanning this body
  Return(Completion),       // stop this scan; bag holds Return | Throw | TailCall | …
}
```

`Activation` / `VmCallStack` exist beside `machine.rs`. Ordinary `Op::Call` does **not** push them. `Op::Call` is still `vm_dispatch::run_call` → `vm_ops::execute_call` → `functions::execute_target` (host recursion). Confirmed against the current tree.

`run_op` (`vm_dispatch.rs`) maps:

| Producer | Live `Transition` |
|---|---|
| hot / simple / dispatch void op | `Continue` |
| simple / dispatch `Some(value)` | `Return(Completion::Return(value))` |
| control `Completion::Normal` (Try / Loop / Branch / Conditional after writing `dst`) | `Continue` |
| control any other `Completion` (`Return`, **`Throw`**, `TailCall`, `Break`, `Continue`, `Yield`, `Suspend`) | `Return(that completion)` |
| `Op::Call` | `Continue` after a recursive host call |
| fall-off of `ops[start..]` | loop returns `Completion::Normal` |

`vm_runtime::completion_result` is `Completion::into_vm_error`. That maps `Normal` → `VmError::MissingReturn`. Script / CJS entry (`quench-node` `run_script` → `execute_with_context` → `execute_in_environment` → `completion_result`) therefore fails when the last op is a statement `Call` (or any other fall-off).

**Probe (reproduced):** `./target/debug/quench-node-cli /tmp/probe_arraysem.js` prints `MissingReturn` and exits 1. Last statement is `console.log("OK array-semantics")`. Workspace tests can still pass: `execute_frames` / `execute_frame_value` now map `Completion::Normal` → `Value::Undefined`, but script/host entry and `functions_receiver::execute_target_with_receiver` still call `completion_result`.

`Op::Throw` is **not** `Transition::Throw`. It is `Transition::Return(Completion::Throw(value))`. There is no isolate loop, no `Push` / `Tail` / `Jump` / `Host`.

Do not describe this shim as stackless. Phase 1 is blocked on the stage contracts below **and** on the probe printing `OK array-semantics`.

### Stage-by-stage contract — Call / Return / Throw

These stages are the Phase 1 cutover. Later phases (tail, control frames, construct, await, host kernels) must not start while R0 is red. Each stage is a contract, not a claim about the current tree.

#### Stage R0 — implicit return is `undefined`, never `MissingReturn`

Fall-off of a script, function, eval, or host-entered body is an implicit JS return of `undefined`. `Completion::Normal` at an **activation boundary** is that implicit return. `Completion::Normal` **inside** a body (control op finished, write `dst`, keep going) stays `Transition::Continue`.

`completion_result` / `into_vm_error` is a semantic decoder for abrupt `Completion`s. It is **not** the activation-boundary applicator. Every host entry that currently does `completion_result(run_ops_completion(…))` must apply the same Normal → `Undefined` rule:

- `vm_execution.rs::{execute_in_environment, execute_in_current_context, run_ops}`
- `functions_receiver.rs::execute_target_with_receiver`
- `functions_arguments_execution.rs::{execute_frames, execute_frame_value}` (already maps Normal → `Undefined`; keep)

**Accept:** `/tmp/probe_arraysem.js` prints `OK array-semantics` and exits 0. A function whose body is only `if (!cond) throw …` returns `undefined`. A script whose last statement is `console.log(…)` succeeds.

#### Stage R1 — `Return` carries a value, not a `Completion` bag

Target arm: `Transition::Return { value }`.

- `Op::Return { src }` → `Return { value: registers[src] }`.
- Activation fall-off (R0) → `Return { value: Undefined }`.
- `ActivationKind::Construct` / `ReturnDest::Construct` / `SuperConstruct` still run `construct_function` / `finish_derived_construct` object-or-`this` selection. They do not blindly return the register.
- Mid-body control `Normal` must not become `Return`.
- **Forbidden:** `Transition::Return(Completion::Throw | TailCall | Yield | Suspend | Break | Continue)`. Those are other arms (T0, Phase 1b, Phase 2/4).

**Accept:** explicit `return 1` writes `1` into the caller dest. Implicit return writes `undefined`. `into_vm_error(Normal)` is no longer reachable from a finished activation.

#### Stage T0 — `Throw` is a distinct transition

Target arm: `Transition::Throw { value }`.

- `Op::Throw { src }` → `Throw { value: registers[src] }`.
- `VmError::Thrown` at the op edge → the same `Throw` (`Completion::from_vm_error` already classifies this).
- Until Phase 2 installs `Frame::Try`, a throw **pops the current activation** and delivers `Completion::Throw` to the caller (same observable as today’s uncaught `Completion::Throw` inside that body). It does not walk try frames yet.
- Uncaught throw at the isolate / script root is still `VmError::Thrown`, never `MissingReturn`, never process abort.

**Accept:** `function f(){ throw 1 }` and a top-level `throw 1` are `Thrown`. `try`/`catch` still uses recursive `exceptions::execute` until Phase 2; do not pretend otherwise.

#### Stage C0 — documented host-recursive Call (current, debt)

Live path, not a destination:

```text
Op::Call → run_call → vm_ops::execute_call → invoke_with_receiver
        → functions::execute_target          // TLS CallDepthGuard, host frame
```

C0 may remain only while R0 is being fixed. Shipping C0 as “stackless Call” fails this ADR.

#### Stage C1 — `Call` returns `Push`, never `execute_target`

Target arm: `Transition::Push { activation }`.

- `Value::Function`, sync, non-generator → push `ActivationKind::Call` with `ReturnDest::Register { dst }` (or the matching dest for `CallMethod`). Park caller `pc` **past** the call op. Check `activations.len() >= MAX_FRAMES` **before** push → `Throw(RangeError("Maximum call stack size exceeded"))`.
- `Builtin` / `HostCapability` may still run in-process (no JS body) or return `Host`.
- `BoundFunction` flattens (`flatten_bound_target`) then follows the target.
- `Proxy` → `Host { kind: Trap { "apply" } }` or Push the target; no Rust recursion through `proxy_apply`.
- `execute_target` for `Value::Function` remains **only** as isolate/host entry: Push + `run_isolate`. It is not the `Op::Call` implementation.

**Accept:** `function d(){ return d() }` throws the RangeError without host overflow. Nested `Op::Call` does not add a Rust frame. Mutual recursion, `arguments`, sloppy `this` match today.

#### Stage C2 — Return applies dest and pops

`run_isolate` on `Return { value }`:

1. Pop the current activation.
2. Apply `ReturnDest` (register write, construct finish, eval dest, promise/generator/host resume, or discard).
3. Resume the caller activation at its parked `pc`. Empty activation stack → isolate/script result is that value (R0 if implicit).

A callee’s R0 implicit return is `Undefined` in the caller dest, not `MissingReturn`.

**Accept:** empty `function f(){}` called from `function g(){ return f(); }` yields `undefined`. Construct dests still obey object/`this` selection.

#### Stage T1 — Throw across calls (still no Try walk)

`run_isolate` on `Throw { value }` while only call activations exist:

1. Pop the throwing activation.
2. If a caller remains, continue as `Throw` in that activation (do not write the caller dest).
3. If the stack is empty, isolate result is `VmError::Thrown(value)`.

Phase 2 later inserts the `Frame::Try` walk **before** popping the activation. T1 must not invent a second exception walker.

**Accept:** `function inner(){ throw e } function outer(){ inner() }` rejects with `e`. After Phase 2, `try { inner() } catch` binds `e` without host recursion.

### Phase 0 — types only (scaffolding landed; not a cutover)

- `Activation`, `VmCallStack`, `Transition` live in `vm/activation.rs`. They are incomplete: no `ActivationKind`, no `ReturnDest`, no `Throw`/`Push`/`Tail`/`Host` arms.
- `FrameStack` limit remains control-frame depth, not JS call depth. `MAX_FRAMES` on `VmCallStack` is 40_000 and is unused by `Op::Call`.
- `run_op` **did** change (the two-arm shim above). That change is the MissingReturn regression, not Phase 1.

**Accept (historical):** types compile; `machine.rs` unit tests still pass. **Not accept:** any claim that calls are stackless.

### Phase 1 — JS-to-JS `Call` / `Return` / `Throw` on `Isolate.activations`

Owned files (suggested, later tasks): `functions_arguments.rs`, `functions_arguments_execution.rs`, `functions_receiver.rs`, `vm/vm_ops.rs`, `vm/vm_dispatch.rs`, `vm/vm_runtime.rs`, `vm/vm_execution.rs`, `vm/activation.rs`.

Execute R0 → R1 → T0 → C1 → C2 → T1 in that order. C0 is the start state, not a deliverable.

- `execute_target` for `Value::Function` (non-async, non-generator) becomes `Push` + `run_isolate` **only at the isolate/host boundary** (script entry, listed host kernels).
- `Op::Call` / `vm_ops::execute_call` / `invoke_with_receiver` Function arm return `Transition::Push`, never `execute_target`.
- `Op::Return` / end-of-body `Normal` → `Transition::Return { value }` (R0/R1).
- `Op::Throw` → `Transition::Throw` (T0/T1), walking **call** activations first (no `Try` frames yet).
- Delete `execute_frames`’s claim to be a call stack; keep it only as the `Tail` implementation or delete it in phase 1b.
- `CallDepthGuard` remains as a belt until phase 1b, then becomes `activations.len()`.

**Accept:** R0 probe green; `function d(){ return d() }` throws `RangeError("Maximum call stack size exceeded")` without host overflow. Mutual recursion and `arguments` / sloppy `this` match today. Focused unit tests around `execute` / `build_registers`. Bounded probe only — no Test262 / full bench.


### Phase 1b — tail calls

- `Op::TailCall` → `Transition::Tail` using `flatten_bound_target` / `resolve_function_target` rules.
- Async/generator/proxy/builtin tails as specified above.

**Accept:** existing tail-call fixtures; depth of a strict tail-recursive loop stays 1 activation above the root.

### Phase 2 — flatten control ops onto `machine::Frame`

`exceptions.rs`, `loops.rs`, `branch.rs`, `conditional.rs`, `switch.rs`, `with_scope.rs`, `private_environment.rs`, `classes` static blocks.

- `Op::Try` pushes `Frame::Try` and `Jump`s to `body` range.
- `Op::Loop` / `ForIn` / `ForOf` push `Frame::Iterator` (for-in keys materialized once, same as `for_in_keys`).
- Nested `execute_completion_in_place` removed from these ops.

**Accept:** try/catch/finally, labeled break/continue, for-of `IteratorClose`, yield-in-try generator tests that already exist. A throw from a callee (phase 1) now lands in the caller’s `Frame::Try`.

### Phase 3 — construct, super, methods, eval, proxy

`construct.rs`, `super_scope.rs`, `methods.rs`, `reflect.rs`, `proxy.rs`, `functions_arguments.rs::execute_construct`.

- `Op::Construct` / `CallSuperConstructor` / `CallSuperMethod` / `CallMethod` / `Eval` return `Transition`.
- Proxy trap post-conditions are `HostResume` steps.
- `execute_construct` deleted once `ActivationKind::Construct` applies `ReturnDest::Construct`.

**Accept:** derived constructors, super once-only, proxy construct object invariant, direct vs indirect eval.

### Phase 4 — await / generators / async on the same loop

`vm_ops.rs::execute_await`, `vm_generator_step.rs`, `generator.rs`, `generator_machine.rs`, `promise.rs`.

- Delete the dual stepper. `execute_generator_step` becomes “install continuation + `run_isolate` until Yield/Suspend/Return”.
- Async functions park `Continuation` instead of dropping the frame.
- Microtask drain moves to the isolate job loop, not `execute_await`.

**Accept:** async function awaiting a later-resolved promise (this is currently lost); async generators; `yield*` ; `try`/`finally` around `yield`/`await`.

### Phase 5 — host kernels

Every remaining `execute_target` in `crates/quench-runtime/src` is either:

- isolate/host **entry** (CLI, microtask job, module evaluate), or
- a `HostResume` kernel.

Remove `isolate.enter` if introduced. Remove `CallDepthGuard`.

**Accept:** `Array.prototype.map` of a JS callback that itself maps (bounded depth) does not grow host stack; `Function.prototype.call` / bound functions / `toPrimitive` still match.

### Phase 6 — heap budget / tracing GC / compact `Value`

Roadmap items 2–3 in the previous ADR text. Not required to declare the VM stackless, but required to close RSS. Activations and continuations are roots.

## What is already in place

- Shared mutable array store; dense `set_index_shared`; canonical index parser.
- `CallDepthGuard` safety net (host-stack limited). Ordinary calls still recurse through `execute_target`.
- `machine::{Machine,Frame,FrameStack,RegisterWindow,FunctionCode,CodeStore}` and generator parking. Generators own a `Machine`; ordinary Call/Construct/eval do not.
- Incomplete `vm/activation.rs`: `Activation`, `VmCallStack`, two-arm `Transition::{Continue, Return(Completion)}`. Not wired as the JS call stack. Not a stackless VM.
- `Completion` / `TailCallRequest` / `LoopTransition`.
- Pre-sized registers in `build_registers`.
- `HOT_DISPATCH` in `vm_dispatch.rs` (stays). The call slot still invokes JS via `execute_target`.
- Unwired `shape_cache.rs` (`ShapeId` / `PropertyId` / `ShapeCache<N>`) and `identity.rs` ABI IDs (`ShapeId`, `PropertyKeyId`). They disagree on ownership — unify before wiring.
- `strings::intern` / `intern_identifier` unused by object lookup. Object writes still COW `Vec<(String, Value)>` + `locals::replace_value`.

## Consequences

Positive:

- Host stack is independent of guest depth; `richards` and infinite recursion become `RangeError` or a Score, not `SIGABRT`.
- `await` / `yield` / `try` / calls share one transition algebra.
- Depth cap is the activation vector, matching Node’s model.

Costs:

- Every call-shaped `Result<Value, VmError>` in the interpreter becomes a `Transition`. That is a large, ordered refactor (phases 1–5).
- Host kernels must be split at JS invocation points.
- Until phase 4, async non-generators remain the existing drain/drop behavior; do not advertise them as stackless.

## Per-suite hotspot map

`tools/bench-perf-index.cjs` is the canonical checker. Full suite runs are **forbidden** in this cluster; the table is diagnostic context only.

| Suite | Result | Dominant cost → primary lever |
|---|---|---|
| richards | SIGABRT stack overflow | Recursive interpreter → **this ADR, phase 1** |
| regexp | validator + recompile | not this ADR |
| deltablue | timeout | property + OO dispatch → shape/IC + stackless |
| raytrace | timeout | property + alloc → shape/IC |
| splay | timeout | property + retention → shape/IC + GC |
| earley-boyer | timeout | per-call frame/Environment → compact activations |
| navier-stokes | timeout | dense numeric → NaN-box + dense arrays |
| crypto | timeout | JSBN → bigint kernel |

## Verification (this design task)

Read-only source evidence only. No `cargo test --workspace`, no Test262, no full bench.

Grounding used: `functions_arguments.rs::{execute_target,execute_construct,build_registers,CallDepthGuard}`, `functions_arguments_execution.rs::{execute,CallFrame,execute_frames,resolve_tail_target}`, `functions_receiver.rs::execute_target_with_receiver`, `vm/vm_runtime.rs::{run_ops_completion_step_from,bare_call_receiver}`, `vm/vm_dispatch.rs::{run_op,run_call,run_tail_call,run_await_completion,run_method_or_construct}`, `vm/vm_ops.rs::{execute_call,invoke_with_receiver,execute_await,prepare_tail_call}`, `vm/vm_execution.rs::execute_frame_completion`, `vm/vm_generator_step.rs`, `completion.rs`, `machine.rs`, `frame_resume.rs`, `identity.rs`, `construct.rs::{execute,construct_function,construct_with_new_target}`, `methods.rs::execute`, `exceptions.rs::execute`, `proxy.rs::{proxy_apply,proxy_construct}`, `super_scope.rs::{execute_call,execute_constructor}`, `reflect.rs::execute_eval`, `generator.rs::{create,resume}`, `generator_machine.rs`, `promise.rs::{from_async_completion,process_async_continuation}`, `docs/architecture.md` (universal continuation machine; rules 91–95 identities).

Later implementation phases use **focused** `cargo test -p quench-runtime --lib <filter>` and micro-probes ≤ 60 s (depth `RangeError`, tail depth, one async resume). They must not run the workspace suite or `quench-bench` full sets.

## Decision detail — shape/slot objects

This cluster is too foundational for a one-task semantic cutover. The current object model encodes identity, descriptors, deleted tombstones, prototypes, and creation order in one `Vec<(String, Value)>` that is cloned on almost every write. A partial swap of storage without freezing the mutation owners would fork semantics. This section is the implementation-ready design: data first, one fact one representation, generic protocol always reachable, no code migration in this turn.

Rules addressed: **31–40** (shape IDs, contiguous slots, deterministic transitions, interned names, dictionary mode, descriptors cold, bounded site states, guard epochs, generic fallback, no second semantics) and **82–86** (invalidation on structural change, accessors generic, proxies generic, dictionary never IC'd, alias identity preserved by in-place mutation).

Stackless VM phases 1–5 above are independent. Shape stages A–H below must not change `Activation` / `Transition`. Heap budget (phase 6) may later replace `Rc<ObjectData>` with `HeapRef` without changing the shape table.

### Current representation (the fact we are replacing)

```text
Value::Object(Rc<ObjectData>)
ObjectData {
  properties: Vec<(String, Value)>,   // data + hidden siblings, last-write-wins via rev()
  created:    Vec<String>,            // enumerable own-key order (own_keys::enumerable_created)
  private_slots: Rc<RefCell<...>>,    // already out of the public key space
  original_prototype: RefCell<...>,
}
FunctionValue.properties / BoundFunctionValue.properties / PromiseData.properties
  = RefCell<Vec<(String, Value)>>     // parallel string maps, in-place via borrow_mut
```

Hidden keys live in the same vector as public data:

| Key | Owner | Meaning |
|---|---|---|
| `\0quench:descriptor:\0{key}` | `builtins::descriptor_key` | full attribute bag (writable/enumerable/configurable/get/set/value) |
| `\0quench:deleted:\0{key}` | `builtins::deleted_key` | own-key tombstone; lookup returns `undefined` without walking proto |
| `\0quench:non_extensible` | `properties.rs` | `[[Extensible]] == false` |
| `\0prototype` | `object.rs` / `classes.rs` | ordinary `[[Prototype]]` |
| `\0function_prototype` | functions | function `[[Prototype]]` |
| `\0home_object` | `object_alias` / `super_scope` | `super` home; often an `ObjectAlias` |
| `\0error_slot`, `\0regexp*`, `\0realm`, `\0quench:module_namespace`, `_value` | various | exotic / boxed / realm / Intl / module flags |

Lookup is a linear `rev()` scan (`vm_object_properties::direct_object_property`, `vm_properties_resolution`, `object_descriptor`). Writes return a **new** `Rc` from `object_alias::set` / `builtins_cells::set_object_property` / `define_own_property` → `define_property_value` → `store_descriptor_metadata` (`Rc::make_mut` + push sibling descriptor). Callers then `locals::replace_value` so registers, environments, and `ObjectAlias` weaks observe the new pointer. Self-reference uses `unsafe` in-place mutation (`object_alias::set` when `value_targets`).

This is one fact stored twice (data slot + descriptor `value`), plus a third copy in `created`. Shape work must collapse that.

### Target representation (one fact)

```text
Heap object (ordinary) =
    shape: ShapeId
    slots: contiguous Vec<Value>     // or later HeapRef-backed slab; index == shape.slot
    proto: Value                     // not a named property
    flags: Extensible | Dictionary
    cold:  Option<ColdProps>         // only if any non-default attribute or accessor

Shape =
    id: ShapeId
    proto_id: Option<ShapeId>        // parent shape, not JS proto
    proto_key: Option<PropertyKeyId> // JS [[Prototype]] identity / epoch, not the value
    proto_epoch: u32
    extensible: bool
    kind: Fast | Dictionary
    entries: [(PropertyKeyId, Slot)] // insertion order == creation order
    trans:  Map<(PropertyKeyId, Attr), ShapeId>  // deterministic add/reconfigure

Slot = { index: u16, attrs: packed W/E/C/kind }
Attr = Writable | Enumerable | Configurable | Data | Accessor

ColdProps = {
    accessors: Map<PropertyKeyId, {get, set}>,   // never in hot slots
    deleted:   Set<PropertyKeyId>,               // dictionary / tombstone
    extras:    Map<PropertyKeyId, Value>,        // dictionary storage
}

PropertyKey = PropertyKeyId(u32)   // interned; symbols and strings share the ID space
Site        = Cold | Mono { shape, key, slot } | BoundedPoly<N=4> | Generic
```

Invariants:

1. **Shape IDs are identity.** Two objects with the same `ShapeId` have the same key→slot map, the same attribute bits, the same extensible bit, and the same `[[Prototype]]` *shape-epoch*. Values in slots may differ.
2. **Slots are contiguous and dense.** Slot `i` is `slots[i]`. No holes in Fast mode. Adding a property appends. Capacity may grow; indices never renumber except on an explicit rebuild that mints a new shape.
3. **Transitions are deterministic.** `(from, key, attrs)` hashes to exactly one `to`. The table is append-only for a given key sequence. Replaying the same add order from the empty root yields the same `ShapeId`.
4. **Names are interned.** Hot comparison is `PropertyKeyId` equality. `strings::intern` is the seed; wrap it as `PropertyKeyId` (do not keep a second `shape_cache::PropertyId` or `properties::StringId`). Arbitrary computed keys intern on first use; identifier-like keys intern at reduce time.
5. **Dictionary mode is the overflow representation, not a second semantic object.** Same `GetOwnProperty` / `[[Set]]` / `[[Delete]]` / `[[OwnPropertyKeys]]` protocol. Fast→Dictionary is a one-way object transition except for a measured rebuild (not required for the first slice).
6. **Descriptors are cold.** Default W/E/C data properties store only the value in `slots`. Non-default attributes and accessors live in `ColdProps` and in the shape's packed `Attr`. `Object.getOwnPropertyDescriptor` *allocates* a descriptor object; it is not a resident sibling key.
7. **Hidden engine slots are not properties.** `[[Prototype]]`, `[[Extensible]]`, private elements, regexp/intl/error/module flags become dedicated fields or a side table. They must leave the public key vector before Fast mode can be the source of truth.
8. **The generic protocol remains the spec.** Fast/IC paths are `guard → typed kernel → canonical fallback`. No IC may implement a different `[[Get]]`/`[[Set]]`.

Dictionary triggers (any one is sufficient):

- own named property count > `MAX_FAST_PROPERTIES` (start at **128**, keeping the Fast table small; object-property hard cap remains `2^16`);
- `[[Delete]]` of a non-last Fast property;
- add of a non-internable / non-identifier computed key after the object is already megamorphic at the site (site goes Generic; object may stay Fast);
- first accessor, first non-default attribute mix that would require a unique shape with no reuse, *or* more than `MAX_SHAPE_TRANSITIONS` (start at **1024**) from this root;
- `BindingCell`-backed properties, module-namespace objects, boxed primitives (`_value`), child-realm globals.

Last-property delete **may** follow the reverse transition (`to.proto_id`) without going Dictionary, because slot length shrinks by one and creation order stays a prefix. Do not implement reverse transitions in the first slice; send every delete to Dictionary and keep the slow path.

### Mutation-path map (owners that must stay single-writer)

Every structural change funnels through one of these owners today. Shape stages may add a helper they all call; they must not grow a second writer.

| Owner | Files | Today | Shape consequence |
|---|---|---|---|
| Residual get | `properties_methods.rs::execute_get`, `properties.rs::execute_get_dynamic` | `get_property_result` | First IC attach point (static `GetProperty` only) |
| Residual set | `properties.rs::execute_set_property` → `finish_set_property` | accessor / builtin / `ordinary_set` | Overwrite Fast data in place; add goes through transition |
| Ordinary set | `properties_reflect_set.rs::set_with_receiver` / `set_receiver_data` / `ordinary_set` | descriptor walk + `define_own_property` + `replace_value` | Fast overwrite skips `define_own_property`; still validates writable |
| Define | `property_define.rs::execute`, `builtins_property_helpers.rs::define_property` / `define_own_property`, `builtins_define_properties.rs` | complete descriptor, placeholder accessor, `store_descriptor_metadata` | Only writer that may mint accessor shapes / cold attrs |
| Cell / alias write | `builtins/builtins_cells.rs`, `builtins/object_alias.rs` | clone vec or BindingCell store; self-ref `unsafe` | BindingCell ⇒ Dictionary. Alias clone **retires** once slots are in-place |
| Delete | `properties_delete.rs`, `builtins_array.rs::delete_*` | new `Rc` + tombstone key | Dictionary (first slice) or reverse-last (later) |
| Integrity | `properties_integrity.rs` | clone `ObjectData`, rewrite every descriptor, mark `\0quench:non_extensible` | New shape with `extensible=false` and attrs cleared; one transition per integrity level |
| Proto | `builtins/object.rs::set_prototype_of` | write `\0prototype` (cell or `set_property`) | New shape family (proto epoch++); invalidate Mono sites that guarded proto |
| Resolve | `vm/vm_properties_resolution.rs`, `vm/vm_object_properties.rs` | type-case + linear scan + proto walk | Fast: `shape.lookup(key) → slots[i]`. Miss → proto walk with proto-shape guard. Proxy/accessor/global stay here |
| Enumerate | `own_keys.rs` | `created` then `ordered` (indices then strings) | Fast: shape entry order, indices first as now. Dictionary: insertion list |
| Builtins | `builtins.rs`, `builtins/object.rs`, `builtins_object_core.rs` | `Object()`, boxed `_value`, intrinsic override table | `Object()` = empty Fast shape. Boxed primitives stay Dictionary/exotic. Intrinsics stay override table |
| Functions / promises / bound | `value.rs` RefCell maps | already in-place, still string-keyed | **Out of first slice.** Same protocol later; do not share `ObjectData` shapes with functions in stage 1 |
| Arrays | `value_array_data.rs`, `builtins_array.rs` | shared element store + named property vec | Elements are the DenseArrays cluster. Named array properties may Dictionary or reuse object shapes **after** ordinary objects |
| Proxies | `proxy.rs` / `proxy_set.rs` | trap early-outs already exist | Never enter shape paths (rule 84) |

`locals::replace_value` / `REPLACEMENTS` / `resolved_replacement` remain until **all** ordinary-object writers mutate in place. The first IC slice must not depend on them for correctness of the get; it may still see them on mixed writes.

### Invalidation boundaries

A site cache entry is valid only while every recorded guard holds. Guards are data:

```text
MonoGuard = {
  receiver_kind: OrdinaryObject,
  shape: ShapeId,
  key: PropertyKeyId,
  slot: u16,
  proto_epoch: u32,          // only if the hit was inherited
  realm_epoch: u32,          // global / intrinsic objects
}
```

| Event | Object | Sites |
|---|---|---|
| Overwrite existing writable data | same `ShapeId`, slot store | **no invalidation** |
| Add new data property | `shape = trans(shape, key, default_attrs)` | previous Mono for *this* object still hits if it named an old key (shape mismatch → miss → re-fill). Sites that cached "absent" must be Generic or carry proto-epoch |
| Delete / non-last structural change | `kind = Dictionary`, new `ShapeId` | every Mono on the old shape misses |
| Data ↔ accessor, or W/E/C change | new shape (attrs in shape) or Dictionary | miss |
| `setPrototypeOf` | new shape + `proto_epoch++` on the old family | inherited Mono misses; own-property Mono on the *new* shape may re-fill |
| `preventExtensions` / `seal` / `freeze` | new shape (`extensible=false`, attrs) | miss |
| Proxy, host exotic, module namespace | never Fast | never cached |
| Intrinsic override / deleted prototype method | realm epoch++ | inherited Mono misses |
| `ObjectAlias` retarget / `replace_value` | identity of the `Rc` changes today | after in-place migration this event **disappears** for ordinary objects |

Miss policy: refill Mono if the new observation is still Fast data; on a second distinct `ShapeId` at the same site, collapse to Generic for the first slice (BoundedPoly is a later measured step; `ShapeCache<4>` already exists for it). A Generic site never grows.

### Accessors, proxies, dictionary — required fallbacks

- **Accessors.** Detected by `property_define::accessor` / descriptor `get`/`set` today. Fast kernel refuses them. `[[Get]]` / `[[Set]]` go to `invoke_accessor` / setter `execute_target` unchanged. Installing an accessor through `define_own_property` either Dictionaries the object or mints an accessor shape whose slot is unused and whose `ColdProps.accessors` holds the pair. First slice: **Dictionary on first accessor.**
- **Proxies.** `early_property_result` and `define_own_property` already branch to `proxy_get` / `proxy_set` / `proxy_define_property`. Shape code is unreachable from `Value::Proxy`.
- **Dictionary.** `slots` unused or empty; `ColdProps.extras` is an insertion-ordered map of `PropertyKeyId → Value`. Lookup is hash + order list. No IC. Same `define_own_property` validation (`validate_redefinition`, non-extensible, array length) runs before the store.
- **Global / host / boxed / arguments / typed array / builtin.** Stay on `vm_properties_resolution` special cases. Do not put the realm global in Fast mode in the first slice (host values + immutable globals + child-realm `\0realm`).

### Alias semantics

JS identity is pointer identity of `Rc<ObjectData>` (`equality.rs`, `same_identity`). Today's write clones that pointer and then *pretends* it did not by rewriting every root. Shape objects invert this:

1. **Ordinary writes mutate `slots` in place.** `Rc` identity is stable. `replace_value` is not consulted. Aliased registers and `ObjectAlias` weaks already point at the same object.
2. **`object_alias::set` clone path is deleted only after (1) is true for add + overwrite + descriptor install.** Until then both representations must not be active on the same object.
3. **`ObjectAlias` remains** as a weak back-reference for `\0home_object` / `super` and for cycle-safe self-fields **until** `HeapRef` exists. It must not be a second property store. `resolve_object_alias` stays the only upgrade.
4. **Self-reference** (`obj.x = obj`) becomes a normal slot store of `Value::Object(same Rc)`. The `unsafe` `Rc::as_ptr` write in `object_alias::set` is not carried forward.
5. **`BindingCell`** stays a Dictionary/exotic feature (arguments mapping, some proto cells). Fast slots never hold a cell; they hold the public value. Cross-cluster: do not invent a third sharing mechanism.
6. **Creation order** is shape entry order (Fast) or Dictionary insertion list. `created: Vec<String>` is derived and then deleted. `object_tests::define_own_property_keeps_creation_order` is the contract.

Until HeapRef/GC, `Rc<ObjectData>` remains the handle. Shapes do not require tracing GC; they require stable identity and a process- or isolate-scoped shape table.

### First measurable monomorphic cache slice

**In scope**

- Receiver: `Value::Object` in Fast mode, not global, not boxed, not namespace.
- Key: interned identifier from `Op::GetProperty { key }` (static). Not computed, not index, not symbol-required.
- Hit: own data property, default or shape-packed attrs, not accessor, not deleted.
- Site: `Cold → Mono` stored beside bytecode (`FunctionCode` side table indexed by op offset), **not** a new `Op` variant and not a field on the 152-byte `Op` enum.
- Kernel: `if object.shape == guard.shape { dst = object.slots[guard.slot] } else { generic }`.
- Miss: `vm_properties_resolution::get_property_with_receiver` unchanged; on Fast own hit, fill Mono.

**Out of scope for the first slice**

- `GetPropertyDynamic`, `SetProperty*`, proto-chain hits, `BoundedPoly`, functions/arrays/builtins, descriptor materialization, delete reverse-transitions, NaN-box slot packing.

**Why this slice is measurable.** deltablue/raytrace/splay are dominated by repeated own `.field` loads on objects that share a constructor-driven add order. The slice removes the `rev()` scan and the `descriptor_key` sibling probe from that path without touching `[[Set]]` identity.

### Staged migration (safe order)

Each stage keeps the previous representation as the fallback and ships with the probes in the next subsection. No stage may remove `replace_value` until stage F (overwrite **and** add are in-place).

| Stage | Change | Source of truth | Fallback |
|---|---|---|---|
| **A — keys** | `PropertyKeyId` newtype over `strings::intern`. Reduce-time intern for static `GetProperty`/`SetProperty` keys. Unify `identity::ShapeId` with `shape_cache::ShapeId`; delete `properties::StringId`. | still `Vec<(String, Value)>` | string compare |
| **B — shadow shapes** | Isolate/process `ShapeTable`. Empty root + deterministic add transitions recorded **after** a successful generic write. `ObjectData.shape: Option<ShapeId>` is advisory. | still the vec | ignore shape on mismatch |
| **C — hidden keys out** | Move `[[Prototype]]`, `[[Extensible]]`, deleted-set, and `\0quench:descriptor:*` into fields / `ColdProps`. Public vec contains only public names. Own-key order comes from `created` until D. | vec of public names + cold table | descriptor helpers read both |
| **D — slots** | Fast objects grow `slots: Vec<Value>` parallel to the public vec. Writes update both. Reads may use slots when `shape` is `Some` and lengths match. | dual | vec on mismatch |
| **E — in-place overwrite** | Existing writable Fast data: store to `slots[i]` (and vec, until F) **without** cloning `ObjectData`. `set_receiver_data` gains this branch next to the dense-array `set_index_shared` fast path. | slots for overwrite | `define_own_property` on miss / accessor / non-writable |
| **F — add transitions + Dictionary** | Adding a key appends a slot and follows `trans`. Overflow / delete / accessor → Dictionary (`ColdProps.extras`). Stop cloning on Fast add. | slots or dict | generic define |
| **G — drop the public vec** | Fast objects no longer carry `Vec<(String, Value)>`. `created` derived from shape. `Deref` to the vec is deleted; every walker listed above is ported. | slots + shape | Dictionary / generic |
| **H — Mono IC** | First slice above. `ShapeCache<1>` per static get site. | slots | `get_property_with_receiver` |

Do not start G or H until E is proven by the identity probe (aliased `obj.x = 1` visible without `replace_value`). Do not start E until C has removed descriptor siblings from the hot vec (otherwise in-place overwrite desyncs `store_descriptor_metadata`).

Functions, bound functions, promises, arrays' named properties, and builtins enter the same table only after G for ordinary objects.

### Implementation-ready tests and bounded probes

No full workspace suite, no test262, no v8-v7 run. Each later implementation task owns the matching slice.

**Table / transition (stage B, unit, no VM)**

- Same add sequence (`a`, then `b`, default attrs) from the empty root ⇒ identical `ShapeId`.
- Different order (`b` then `a`) ⇒ different `ShapeId`.
- Same key + different attrs ⇒ different `ShapeId`.
- Transition map is a function: inserting twice does not allocate a third shape.
- Dictionary flag objects never appear in the Fast transition map.

**Semantic contracts already in-tree (must keep passing, focused)**

- `builtins/object_tests.rs::define_own_property_keeps_creation_order`
- `builtins/object_tests.rs::array_accessor_define_keeps_creation_order`
- `has_own_observes_function_own_properties`
- `get_own_property_descriptor_throws_on_nullish_target`

**Identity / alias / accessor probe (stage E, JS, ≤60s)** — objects only:

```js
const a = {}; const b = a;
a.x = 1; if (b.x !== 1) throw "alias overwrite";
a.y = a; if (b.y !== a) throw "self ref";
Object.defineProperty(a, "z", { get() { return this.x; } });
if (a.z !== 1) throw "accessor";
if (!delete a.y) throw "delete";
if ("y" in a) throw "deleted own";
Object.setPrototypeOf(a, { x: 9 }); // x still own
if (a.x !== 1) throw "own vs proto";
```

Run against `target/debug/quench-node` and Node; require identical throws / values. Accessors, proxies (`new Proxy(a,{get(){return 7}})`), and `Object.freeze` must take the generic path and still match Node.

**Monomorphic get micro-probe (stage H, ≤60s)**

```js
function make() { const o = {}; o.x = 1; o.y = 2; return o; }
const os = Array.from({length: 1000}, make);
let s = 0;
const t0 = Date.now();
for (let i = 0; i < 200000; i++) s += os[i % os.length].x;
const ms = Date.now() - t0;
if (s !== 200000) throw s;
```

Record wall ms + (if available) shape-hit / generic-miss counters on stderr. Accept the slice only if (1) the semantic probe still matches Node and (2) the hit counter shows ≥99% Mono on this workload. Do **not** gate on a wall-time ratio until a before/after record exists on the same binary flags. Forbidden: full `quench-bench` / test262.

### Remaining risks

- `Deref`/`DerefMut` on `ObjectData` leaked the vec into dozens of files; G is a mechanical port, not a behavior change, but it is wide. Stay inside listed owners per task.
- Global objects, host capabilities, and intrinsic override tables impersonate ordinary objects today; putting them on Fast shapes will silently wrong-answer without the epoch guards.
- Dual-write stages (B–E) can drift if any writer updates only one side. The single-writer table above is mandatory; a debug `shape_len == public_len` assert belongs on Fast objects until G.
- `object_alias` `unsafe` and `REPLACEMENTS` are load-bearing for current identity. Removing them before F is a correctness bug, not a cleanup.
- `shape_cache.rs` and `identity.rs` currently disagree on `ShapeId` ownership. Unifying IDs is stage A; leaving both is a second representation.
- Dense array indices and NaN-box `Value` are sibling clusters. This design does not change `ArrayData` or `Value` width.

### Phase A contract (first implementation slice)

This is Stage A in the table above. It is a **design-bound implementation slice for a later task**, not work claimed by this ADR turn. Source of truth stays `ObjectData.properties: Vec<(String, Value)>`. No IC, no `replace_value` removal, no slot vector, no `ObjectData.shape` stamp (that is Stage B).

#### IDs

| Type | Canonical definition | Phase A rule |
|---|---|---|
| `ShapeId(u32)` | `identity.rs` only | Delete the duplicate `shape_cache::ShapeId`. Reserved values are unused in Phase A except as named consts for later stages: `0` empty/unknown, `1` dictionary, `2` accessor, `3` exotic. Do not publish ordinary shapes in Phase A. |
| `PropertyKeyId(u32)` | `identity.rs` only | Delete `shape_cache::PropertyId`. `0` is `PROPERTY_KEY_INVALID` (uninterned / overflow / non-key). Hot equality is integer. |
| `strings::StringId` | identifier pool in `strings.rs` | Seed for identifier-like names only. Do not edit `strings.rs` (string-intern owner). Do not treat `StringId` as `PropertyKeyId`. |
| `properties::StringId` | pointer-identity hint | **Not Phase A.** The Stage A table lists its deletion; that edit lives in `properties.rs`, which a live cache slice owns. A follow-up in the same stage may switch `property_name_eq` to `PropertyKeyId` with string fallback. Until then pointer-identity and `PropertyKeyId` coexist; they are not a second key space in the shape table because Phase A has no shape table. |

`PropertyKeyId` is a newtype **over the intern index**, not a second copy of the bytes. Identifier-like keys (`strings::intern_identifier` / `intern`) may reuse that pool’s `u32` by wrapping it: `PropertyKeyId(string_id.0)` when intern succeeds. Non-identifier keys, symbols, keys `> 256` bytes, and pool overflow return `None` / `PROPERTY_KEY_INVALID` and stay string-compared. Arbitrary computed keys are **not** dumped into the process-wide identifier pool.

Symbols intern later by identity payload (`desc\0id`), not description. Phase A does not intern symbols.

Hidden engine keys (`\0quench:descriptor:\0…`, `\0quench:deleted:\0…`, `\0quench:non_extensible`, `\0prototype`, `\0function_prototype`, `\0home_object`, `_value`, …) are **never** `PropertyKeyId`s. They leave the public key vector in Stage C.

The intern table Phase A adds (if any) is **isolate-owned, budgeted metadata**. It must not grow via a process-global `Mutex<HashMap>` that bypasses `heap_limit`. Wrapping `strings::intern` is allowed only because that pool is already bounded (`16_384` names, `1 MiB`, `256` bytes/name) and already exists; new key storage belongs on the isolate and is charged. Isolate reset invalidates live `PropertyKeyId`s that did not come from the process identifier pool. OOM / cap → `None`, never panic, never a second semantic path.

#### Contiguous slots (binding contract; not allocated in Phase A)

Unchanged from “Target representation” above. Phase A must not allocate `slots: Vec<Value>` or dual-write. The contract later stages are not allowed to reinterpret:

- Fast slot `i` is `slots[i]`; no holes; add appends; indices do not renumber.
- Default W/E/C data properties store only the value in the slot. Accessors and non-default attrs are cold (Stage C / F).
- `created` order for Fast objects **is** shape add order after Stage G. Until then `created: Vec<String>` remains the enumeration fact.
- Private slots stay off this vector.
- Two stores are two facts: dual-write is permitted only in Stages D–E, with the single-writer table, and must be dropped in G.

#### Invalidation (binding contract; no sites in Phase A)

Phase A installs no site cache and no `ObjectData.shape`. The invalidation table in “Invalidation boundaries” is still law for Stages B–H. Phase A-specific consequences:

- Unifying `ShapeId` must not create a cache that get/set consults.
- `shape_cache.rs` stays a disconnected prototype (`ShapeCache<N>`). After the type unify it uses `identity::ShapeId` + `identity::PropertyKeyId`.
- A later Mono fill (Stage H) guards `{ receiver_kind: OrdinaryObject, shape, key, slot, proto_epoch?, realm_epoch? }`. Miss → existing `get_property_with_receiver`.

#### Dictionary / accessor / proxy fallback (binding contract)

Phase A does not mint Dictionary objects. Fallbacks stay the current owners; later stages may only *arrive* here, not replace them:

| Kind | Detection today | Owner that remains generic |
|---|---|---|
| Proxy | `Value::Proxy` | `proxy_get` / `proxy_set` / `proxy_delete` / `proxy_define_property` |
| Accessor | `property_define::accessor` / descriptor `get`/`set` | `finish_set_property`, `set_with_receiver`, `assign_set_property` |
| Tombstone / integrity / non-default attrs | `deleted_key`, `\0quench:non_extensible`, `descriptor_key` | `define_own_property`, `delete_property`, `direct_object_property`, `properties_integrity` |
| Dictionary (from Stage F) | `kind = Dictionary` | same `define_own_property` validation; `ColdProps.extras`; **no IC** |
| Global / boxed / namespace / host | `_value`, realm global, module namespace | `vm_object_properties`, `vm_properties_resolution` |
| Array index | `ArrayData::set_index_shared` | DenseArrays cluster; not this slice |
| Function / bound / promise maps | `RefCell<Vec<(String, Value)>>` | out of Stages A–H for ordinary objects |

First accessor, any delete, BindingCell, boxed `_value`, and the realm global **Dictionary** (or never Fast). That is Stage F policy; Phase A must not pre-implement a HashMap store.

#### Exact Phase A code boundaries

**May add or edit**

| File | Allowed change |
|---|---|
| `crates/quench-runtime/src/identity.rs` | Comments and reserved `ShapeId` / `PropertyKeyId` consts. No new ID newtypes. |
| `crates/quench-runtime/src/property_key.rs` **(new)** | `intern_property_key(&str) -> Option<PropertyKeyId>` and `resolve(PropertyKeyId) -> Option<&str>` (or owned `String` if a borrow across the isolate lock is impossible). Wraps `strings::intern` / `intern_identifier` for identifier-like names. No `Value`, no `ObjectData`, no get/set. |
| `crates/quench-runtime/src/shape_cache.rs` | Drop local `ShapeId` / `PropertyId`; use `identity::{ShapeId, PropertyKeyId}`. Keep capacity/`lookup`/`insert` behavior. Do not call it from the interpreter. |
| `crates/quench-runtime/src/lib.rs` | `mod property_key;` only. No `pub use` that implies objects are shaped. |

In-module `#[cfg(test)]` only: intern hit/miss, invalid id, identifier overflow → `None`, cache type compile after the unify. No `tests/**`.

**Must not edit in Phase A**

- `value.rs` / `ObjectData` / `own_property_cache` (property-cache slice)
- `strings.rs` (string-intern owner)
- `properties.rs` and `properties_*.rs` (StringId deletion and `execute_*` are follow-up / later stages)
- `property_define.rs`, `objects.rs`, reducers, `ops_op.rs` / `Op::GetProperty` layout (key stays `String`)
- `builtins_error_helpers.rs`, `builtins_property_helpers.rs`, `builtins_array.rs`, `builtins.rs`
- `builtins/builtins_cells.rs`, `builtins/object_alias.rs`, `builtins/object.rs`, `builtins/object_*.rs`
- `vm/vm_object_properties.rs`, `vm/vm_properties.rs`, `vm/vm_properties_resolution.rs`, `vm/vm_dispatch.rs`
- `proxy.rs`, `proxy_set.rs`, `execute.rs`
- Array / Function / typed-array / buffer stores
- `tests/**`, `tests/node/**`, `AGENTS.md`, `Cargo.lock`, JIT, crate removals

Reduce-time intern of static `GetProperty` / `SetProperty` keys (Stage A table) is a **follow-up patch in the same stage** once reducer files are free: emit the interned `&str` into the existing `Op` `String` field so pointer-identity `property_name_eq` can hit. It must not change `Op` width or add `GetPropKnownShape`.

**Phase A acceptance (later implementation task)**

- `rg 'struct ShapeId|struct PropertyId|struct PropertyKeyId' crates/quench-runtime/src` shows definitions only in `identity.rs` (plus `strings::StringId`, unrelated).
- `shape_cache` tests still pass after the type unify.
- `property_key` intern tests pass; overflow is `None`.
- No get/set/define/delete/proto/integrity path reads `ShapeId` or `PropertyKeyId`.
- Focused object tests listed in “Implementation-ready tests” still pass; behavior unchanged.
- This ADR’s Stages B–H remain unimplemented.

**Not claimed by this document:** any production object is shaped, any lookup is `shape+slot`, any mutation owner was migrated.

## Decision detail — CPU / cache (rules 81–90)

Physical layout law for later representation work. This section **does not** change `Value`, `Op`, objects, or the interpreter. It records measured sizes, binds SoA/AoS choices to those sizes, and names the only machine fields a probe may emit. Hardware performance counters are **not** inferred.

### Host used for the record

| Fact | Measured 2026-08-22 (re-probe; host facts unchanged from 2026-08-21) |
|---|---|
| Target | `aarch64-apple-darwin`, Apple M4, `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| `sysctl hw.cachelinesize` | **128** |
| `hw.pagesize` | **16384** |
| P-core L1I / L1D / L2 (`hw.perflevel0.*`) | 192 KiB / 128 KiB / 16 MiB |
| E-core L1I / L1D / L2 (`hw.perflevel1.*`) | 128 KiB / 64 KiB / 4 MiB |
| `hw.l1dcachesize` / `hw.l1icachesize` / `hw.l2cachesize` | 64 KiB / 128 KiB / 4 MiB (aggregate / E-core-sized; probe ABI emits these three) |
| `getconf LEVEL1_DCACHE_LINESIZE` | unset on this Darwin |

`hw.cachelinesize = 128` is the OS-reported **coherence / false-share stride** on this Apple Silicon host. It is **not** a claim that every L1 transfer is 128 bytes (Apple L1 lines are commonly 64; this sysctl follows the larger stride). Portable header math uses **64**. Padding and arena alignment use `max(64, measured_line)`.

On Linux, measure `/sys/devices/system/cpu/cpu0/cache/index0/coherency_line_size`. If no OS source exists, assume 64 and label the field `assumed-64-not-measured`. Never invent LLC-miss or IPC numbers.

### Measured `size_of` / `align_of` (release, same rustc)

Throwaway `size_of` probe against `quench-runtime` public types, release profile, same rustc, 2026-08-22. `Activation` is `pub(crate)` and is **derived** from field sizes, not `size_of`. Previous 2026-08-21 table listed `ObjectData` as 144; the live type is **192** because `own_property_cache: RefCell<ShapeCache<4>>` (80) plus `shape_signature` / `shape_id` now sit in the header.

| Type | size | align | Cache lines @64 / @128 |
|---|---:|---:|---|
| `Value` | 32 | 8 | 0.50 / 0.25 |
| `TaggedValue` | 8 | 8 | 0.125 / 0.0625 |
| `ObjectData` | **192** | 8 | **3 / 1.5** |
| `ArrayData` | 168 | 8 | 2.625 / 1.3125 |
| `ArrayBufferData` | 120 | 8 | 1.875 / 0.9375 |
| `FunctionValue` | 104 | 8 | 1.625 / 0.8125 |
| `BoundFunctionValue` | 128 | 8 | 2 / 1 |
| `PromiseData` | 232 | 8 | 3.625 / 1.8125 |
| `Op` | 152 | 8 | 2.375 / 1.1875 |
| `HotOp` | 1 | 1 | (existing `Op::layout` test) |
| `Machine` | 192 | 8 | 3 / 1.5 |
| `Frame` | 80 | 8 | 1.25 / 0.625 |
| `FrameStack` | 40 | 8 | 0.625 / 0.3125 |
| `RegisterWindow` | 32 | 8 | 0.50 / 0.25 |
| `FunctionCode` | 24 | 8 | 0.375 / 0.1875 |
| `Completion` | 88 | 8 | 1.375 / 0.6875 |
| `PackedCompletion` | 12 | 4 | 0.1875 / 0.0938 |
| `CodeRange` | 12 | 4 | 0.1875 / 0.0938 |
| `identity::HeapRef` / `identity::ShapeId` / `PropertyKeyId` / `CodeId` / `EnvironmentRef` / `FrameId` / `ContinuationId` | 4 | 4 | |
| `tagged_value::HeapRef` | 8 | 4 | generation + index; not the identity handle |
| `HeapArena<Value>` | 48 | 8 | 0.75 / 0.375 |
| `(String, Value)` | 56 | 8 | 0.875 / 0.4375 |
| `Vec<Value>` / `String` | 24 | 8 | |
| `Option<Value>` | 32 | 8 | same as `Value` (niche) |
| `ShapeCacheEntry` | 12 | 4 | |
| `ShapeCache<4>` | 72 | 8 | 1.125 / 0.5625 |
| `RefCell<ShapeCache<4>>` | 80 | 8 | 1.25 / 0.625 |
| `RefCell<Option<Value>>` | 40 | 8 | |
| `Activation` (derived) | **104** | 8 | `Rc` 8 + `pc` 8 + `Vec<Value>` 24 + `Rc` 8 + `Value` 32 + `Option<u16>` 4 + `Option<usize>` 16 = 100, pad 104 |

Occupancy on a portable 64-byte line: **2** `Value`s, **8** `TaggedValue`s, **0.42** `Op`s, **0.33** `ObjectData`s. `Value` is 34 variants / 32 bytes. `Op` is 102 variants / 152 bytes. Those match the 2026-08-22 `size_of` probe; a later `size_of` change must update this table and `tools/bench-ops.cjs` `machine.layouts`, not silently keep the old numbers.

### Rule 81 — design around cache lines

- **Portable hot-header budget:** 64 bytes. A hot header that exceeds 64 is a failed layout even on this 128-byte host.
- **Host stride:** `machine.cache_line_bytes` from the probe (128 here). Use it for false-share padding and arena chunk alignment only.
- **Do not** special-case Apple 128 in semantic code. The probe records it; the types stay 64-budgeted.

### Rule 82 — hot object / machine headers stay in one 64-byte line

Current headers fail this:

- `ObjectData` (**192**, three 64-byte lines) is `properties` 24 + `private_slots` 8 + `original_prototype` 40 + `created` 24 + `own_property_cache` **80** + `shape_signature` 8 + `shape_id` 4. The 4-entry IC alone is 1.25 lines. A named load can touch three lines before comparing a key.
- `ArrayData` (168) keeps `length` next to two named-property `Vec`s, arguments maps, and `RefCell<Option<Value>>` prototype. Dense elements are already `Rc<RefCell<Vec<Value>>>` (shared buffer) but still boxed 32-byte `Value`s.
- `Machine` (192) embeds `Completion` (88) beside `pc`. The live cursor is three lines.
- `Op` (152) is the executable stream unit: every `pc += 1` walks 2–3 lines.

**Bound headers** (later representation work; not allocated in this turn):

```text
ObjectHeader (≤32, one line with optional inline slots later)
  shape: ShapeId           // 4
  flags: u32               // 4  Extensible | Dictionary | …
  proto: HeapRef           // 4
  slots: HeapRef           // 4  or later a packed pointer
  slot_len: u16
  slot_cap: u16

ArrayHeader (≤24)
  length: u32
  capacity: u32
  kind: u8                 // packed-int | packed-f64 | packed-value | holey | sparse
  flags: u8
  elements: HeapRef

MachineCursor (≤32, the only per-op load)
  code: CodeId             // 4
  pc: u32                  // 4
  env: EnvironmentRef      // 4
  registers_base: u32      // 4
  registers_count: u16     // 2
  frame_base: u16          // 2
  completion: PackedCompletion  // 12
```

`Machine.completion: Completion` (88) moves off the cursor. `RegisterWindow.values`, `FrameStack.frames`, `store`, and parked `Rc<Environment>` are **not** in the header. `Activation` drops the leftover `receiver: Value` and `registers: Vec<Value>` once the shared register file exists (see stackless section); the bound record is IDs + window, not a 32-byte `Value` plus a heap `Vec`.

### Rule 83 — split hot and cold fields

| Record | Hot (must be in the first line / first vector) | Cold (second allocation or side table) |
|---|---|---|
| Ordinary object | `shape`, flags, `proto` id, slot storage, optional **one** 12-byte monomorphic guard | `private_slots`, `created` (until derived from shape), `ShapeCache<4>` / any N>1 IC, accessors, debug names |
| Array | `length`, `kind`, element buffer | named `properties` / `descriptors`, `mapped` / `deleted`, arguments live record, prototype cell |
| `Op` | opcode + compact operands | source locations, `String` keys, `FunctionCode` bodies, spreads, labels |
| `FunctionValue` | `CodeId`, `params`, `EnvironmentRef`, kind/strict/async bits | `properties` map, `instance_fields`, `with_captures`, `PrivateEnvironment` |
| `PromiseData` | state tag + result `HeapRef` | `then_actions`, `continuations`, named properties |
| `Machine` | cursor above | `Frame` payloads, `CodeStore` pointer, semantic `Completion` |
| Arithmetic | immediates / `TaggedValue` bits | prototype walks, finalizers, host capability metadata |

Loading `ObjectData.original_prototype`, `ObjectData.own_property_cache` (80 B), or `FunctionValue.private_environment` from an add/get-index path is a layout bug, not an API convenience. The inline 4-wide cache is the current defect that grew the header from the prior 144-byte record to 192.

### Rules 84–85 — SoA vs AoS (decided from the 2026-08-22 table, not taste)

| Data | Today | Decision | Why |
|---|---|---|---|
| Executable ops | AoS `Vec<Op>` @ 152 B | **SoA:** `opcodes: [u8]` (or `u16` if the set grows) + operand/side tables. Dispatch metadata must be the **only** hot fetch | Dispatch / `pc++` touches one field across many entries (84). Rare `String` / `FunctionCode` must not ride every instruction (15, 83). 152 B ⇒ 0.42 ops/line |
| Dense numeric elements | shared `Rc<RefCell<Vec<Value>>>` @ 32 B/elem | **SoA / packed:** `f64[]` or `i32[]` while `numeric_shape` holds; boxed `Value[]` only after kind escape | Kernels (`a[i] + 1`) touch one payload across many indices (84). Sharing the `Vec` removed clone traffic; it did not pack the payload |
| Dense value / object slots | `Vec<(String, Value)>` @ 56 B | **AoS of `Value` slots** addressed by `ShapeId` + slot; names live in the shape, not beside each value | A get consumes the whole slot (85). The pair layout pays a 24-byte `String` on every neighbor |
| Register file | `Vec<Value>` | **AoS of one-word values** (today 32 B; target 8 B `TaggedValue` / identity `HeapRef`) | Call/arithmetic consume the whole register (85). 32 B ⇒ 2 regs/line; 8 B ⇒ 8 |
| Activation / `Frame` | AoS 104 / 80 B | **AoS** of the compact record | Push/pop/unwind consume the whole record (85). Do not SoA `pc[]` separately from `function[]` |
| Shape IC | inline `ShapeCache<4>` @ 72 B inside `ObjectData` | **Side table**, or at most one 12-byte `ShapeCacheEntry` in the header | A lookup walks one `(shape, property)` column (84). 72 B in the object header fails 82 |
| Shape transition table | n/a (Phase A has none) | **SoA of `(PropertyKeyId, Attr, ShapeId)`** once the table exists | Lookup walks one key column (84) |
| Isolate / worker counters | `thread_local!` `Cell`s | **SoA of padded scalars**, one stride per isolate | One field (`bytes`, `ops`) incremented across workers (84, 88) |

Default: **AoS for records the interpreter consumes whole; SoA for streams scanned by one field.** Mixing both in one type (today’s `Op` @ 152, `ObjectData` @ 192, `ArrayData` @ 168) is the defect.

### Rule 86 — cut dependent pointer loads

Today a named own-property hit is:

```text
Value::Object(Rc) → ObjectData (192 B, 3 lines) → properties: Vec<(String, Value)> → String bytes → compare → Value
```

Four dependent loads, then a heap string. The 80-byte `own_property_cache` sits in the same allocation but is not on this hit path unless the signature is stale. Target:

```text
Value / TaggedValue → ObjectHeader (shape + slots) → slots[i]
```

Two loads after tag decode. Shape compare is an integer (`identity::ShapeId`, 4 B — not `tagged_value::HeapRef` at 8 B). `Rc<RefCell<Vec<Value>>>` on `ArrayData.values` is one extra hop versus a packed `HeapRef` + index: clone/share is cheap, indexed numeric kernels still chase `RefCell` then `Value`. Environment `BindingRef → SlotStore → RefCell<Vec<Value>>` and `FunctionCode → OnceLock → CodeStore → Vec<Op>` are the same class of chain; they shrink to `HeapRef` + index when those clusters land. Do not add a new pointer between the cursor and the opcode byte.

### Rule 87 — no atomics inside an isolate

The isolate is single-threaded. Observed atomics / locks today:

| Site | Role | Decision |
|---|---|---|
| `atomics.rs` + `Builtin::Atomics*` | Guest `SharedArrayBuffer` | Keep. This is JS `Atomics`, not engine metadata |
| `strings.rs` `Mutex<StringPool>` | Process intern | Cold. Migration is isolate-owned intern (string cluster), not `AtomicU32` in the interpreter |
| `regexp.rs` `RwLock<HashMap>` | Process regex cache | Cold. Charge/evict under the isolate budget; no atomic in the match kernel |
| `intl` `AtomicU64` symbol counter | Identity mint | Stay out of arithmetic / property paths |
| `FunctionCode` `OnceLock<Rc<CodeStore>>` | Link-once | Allowed as init-once, not a per-op acquire |
| Interpreter, shapes, heap, activations, site ICs | — | **`Cell` / `RefCell` / exclusive `&mut` only.** No `Atomic*`, no `Mutex` on the JS path |

`Send`/`Sync` on isolate state is a bug unless the type is guest SAB memory or a documented process table.

### Rule 88 — false sharing

There is no shared worker-stat block today (`CALL_DEPTH`, `ARRAY_BUFFER_BYTES` are `thread_local!`). When isolate/worker counters exist:

- Stride = `max(64, machine.cache_line_bytes)` (128 on this host).
- One counter group per isolate, aligned to that stride. Adjacent isolates must not share a line.
- Do not pad `Value`, `Op`, or slots to 64/128. Padding is for **shared writers**, not every record.

`tools/bench-ops.cjs` may run a two-worker adjacent-vs-padded `Int32` increment **only** as a wall-time probe. It must not report synthetic miss counts.

### Rule 89 — align arenas and pages, not every value

- Heap / opcode / slot / nursery chunks: aligned to `machine.page_bytes` (16384 here) or a documented 4096/16384 size class.
- Metadata arenas (shapes, intern, IC sites): aligned to the false-share stride.
- `Value` stays `align(8)` (or the tagged-word align). Over-aligning values wastes the line the header is trying to keep.
- `HeapArena<T>` (`Vec<Option<T>>`, 48-byte struct) is a free-list of pointers, not a page. A later slab must replace it before alignment claims apply to objects.

### Rule 90 — no manual prefetch

**Forbidden** until a recorded hardware-counter profile (rule 99; not this cluster) shows a sequential scan whose LLC/L2 misses drop under an explicit prefetch and whose wall time drops on the same binary flags.

`tools/bench-ops.cjs` compares sequential vs random `Float64Array` **wall time only**. A sequential win is evidence that the *hardware* prefetcher is doing its job; it is **not** authorization to emit `prefetch` / `_mm_prefetch` / `intrinsics::prefetch`. Default `machine.prefetch_policy = "forbidden-until-profiled"`.

### Machine-level fields (probe ABI)

`tools/bench-ops.cjs` emits a `machine` object. Allowed keys:

| Field | Source |
|---|---|
| `arch`, `platform` | `os.arch()`, `os.platform()` |
| `cache_line_bytes`, `cache_line_source` | `sysctl hw.cachelinesize` / sysfs / `assumed-64-not-measured` |
| `page_bytes` | `os` / `sysctl hw.pagesize` / `getconf PAGE_SIZE` / `assumed-4096-not-measured` |
| `l1d_bytes`, `l1i_bytes`, `l2_bytes` | Darwin `hw.l1dcachesize` / `hw.l1icachesize` / `hw.l2cachesize` or `null` |
| `portable_header_budget_bytes` | constant `64` |
| `false_share_stride_bytes` | `max(64, cache_line_bytes)` |
| `prefetch_policy` | `"forbidden-until-profiled"` |
| `layouts` | static table from the rustc `size_of` record above, plus `layout_target` / `layout_measured_at` |
| `occupancy` | derived from `layouts` / 64: values, tagged words, ops, object headers per portable line |
| `decisions` | the SoA/AoS/header verbs in this section |

**Forbidden fields:** cycles, instructions, IPC, branch misses, L1/L2/LLC misses, TLB misses, `perf`, guessed `cache_misses`. `allocs_proxy` remains the existing heap-delta/64 estimate and must stay labeled a proxy.

Repeatable cache probes (same harness, smaller iteration cap, still inside `MAX_TOTAL_MS`):

1. `soa-field-scan` vs `aos-record-scan` — one numeric field across N entries (84).
2. `aos-whole-record` vs `soa-whole-record` — consume every field of a 3-wide record (85).
3. `aos-value32-scan` vs `soa-f64-scan` — 32-byte `Value` stride vs packed `f64` (84, arrays).
4. `header-hot-prefix` vs `header-cold-tail` — first word vs last word of a 192-byte record (`ObjectData` / `Machine` span) (82, 83).
5. `sequential-f64` vs `random-f64` — hardware-prefetcher hint; no software prefetch (90).
6. `index-chase` vs `pointer-chase` — packed `next[]` vs linked objects (86).
7. `false-share-adjacent` vs `false-share-padded` — two workers, `SharedArrayBuffer`, wall time only (88).

Focused wall-time medians on this host (`CACHE_ITERS=40000`, two repeats, working sets 4–64 KiB, L1/L2 resident). These are **hints**, not hardware counters and not a license to change Rust layouts in this cluster:

| Pair | Median ns | Reading |
|---|---:|---|
| `soa-field-scan` / `aos-record-scan` | 93625 / 117750 | One-field scan prefers SoA (84) |
| `soa-whole-record` / `aos-whole-record` | 117250 / 134417 | JS object AoS still loses to typed SoA; Activation/`Frame` stay AoS because the *Rust* record is consumed whole, not because this JS probe won |
| `aos-value32-scan` / `soa-f64-scan` | 98333 / 98375 | Tied at this working set. Packing is decided by occupancy (2 vs 8 values/line), not this delta |
| `header-hot-prefix` / `header-cold-tail` | 98042 / 112417 | Touching the last word of a 192-byte record is slower; keep ICs off the first line (82, 83) |
| `sequential-f64` / `random-f64` | 97667 / 102583 | Sequential wins without software prefetch (90) |
| `pointer-chase` / `index-chase` | 109209 / 125250 | Pointer won while the 4096-node set stays cached; does **not** reverse 86 for heap-scale graphs |
| `false-share-padded` / `false-share-adjacent` | 533334 / 1055041 | 128-byte stride ~2× (88) |


### Budget / accounting

Opcode SoA side tables, shape tables, site ICs, and padded counter slabs are **isolate-budgeted metadata**. They count toward the heap cap and must be droppable on isolate reset. They are not a process-global unaccounted cache. Cross-cluster: Memory/Heap accounting owns the charge; this section only forbids hiding them.

### Not in this turn

No `#[inline]` / `#[cold]` primitive is added. Existing `#[inline]` on `HotOp::id` / `hot_dispatch` is the other cluster (91–95). No prefetch intrinsic. No `repr(align(64))` on `Value`/`Op`. No change to `ObjectData` / `ArrayData` / `Machine` layouts here — those land with their owning clusters against the headers above.


## See also

- ADR 0001: Node 24 broad compatibility runtime contract
- ADR 0002: pluggable engine / quench-node scope
- `docs/architecture.md` — `Machine`, packed completion, universal continuation, `Frame` phases, heap object = shape ID + packed slots
- `docs/compatibility-contract.md`
- `docs/data-first-minimal-runtime.md`
