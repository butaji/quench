# Task 053: Rust Function Callbacks (60fps Critical)

## Goal
Replace string-based JS callbacks with rquickjs `Function` references for true 60fps performance.

## The Problem

**Current architecture stores JS callbacks as STRINGS:**
```rust
// bridge.rs - Input callbacks
static INPUT_CALLBACKS: ... HashMap<u32, String>

// bridge.rs - Timer callbacks  
struct TimerEntry {
    callback_js: String,  // ← JavaScript CODE STRING
    ...
}

// bridge.rs - Microtask callbacks
struct MicrotaskEntry {
    callback_js: String,  // ← JavaScript CODE STRING
}
```

**What happens on every key press (current):**
1. Key event received in Rust
2. Rust builds JS string: `"(handler)({key:'Enter',ctrl:false,...})"`
3. Rust returns string to main.rs
4. main.rs evals the string: `ctx.eval("try { ... }")`
5. QuickJS parses and executes the code
6. Handler function finally runs

**What happens on every timer tick (current):**
1. 10ms polling timer fires
2. Rust collects due timers
3. Returns `Vec<String>` of callback code
4. main.rs evals each string: `ctx.eval("try { callback_code }")`
5. QuickJS parses and executes

**This is SLOW. String building + parsing + eval for EVERY callback.**

## The Solution

**Store rquickjs `Function` references and call directly:**
```rust
use rquickjs::{Ctx, Function, Value};

// Store Function references instead of strings
static INPUT_HANDLERS: ... HashMap<u32, StoredFunction>

struct StoredFunction<'js> {
    func: Function<'js>,
    ctx: Ctx<'js>,
}

// Call directly — no string building, no eval
pub fn dispatch_key(key: &str, ctrl: bool, shift: bool, alt: bool) {
    for handler in handlers.values() {
        handler.func.call((key, ctrl, shift, alt)).ok();
    }
}
```

**What happens on every key press (target):**
1. Key event received in Rust
2. Rust iterates stored Function refs
3. Rust calls `func.call((key, ctrl, shift, alt))` directly
4. Handler runs immediately

**No string building. No eval. No parsing. Direct C++ → JS function call.**

## Required Changes

### 1. Input handlers (bridge.rs + main.rs)

**Current:**
```rust
// bridge.rs
pub fn __ink_register_input(callback_js: &str) -> u32 { ... }
pub fn __ink_dispatch_key(...) -> String { ... }  // returns JS strings to eval

// main.rs
let callbacks = bridge::__ink_dispatch_key(key, ctrl, shift, alt);
for cb in callbacks { ctx.eval(cb); }
```

**Target:**
```rust
// bridge.rs
pub struct InputRegistry { handlers: HashMap<u32, rquickjs::Function> }
impl InputRegistry {
    pub fn register(&mut self, func: rquickjs::Function) -> u32 { ... }
    pub fn dispatch_key(&self, key: &str, ctrl: bool, shift: bool, alt: bool) { ... }
}

// main.rs
registry.dispatch_key(key_str, ctrl, shift, alt);  // calls Functions directly
```

### 2. Timer callbacks (bridge.rs + main.rs)

**Current:**
```rust
struct TimerEntry {
    callback_js: String,
    ...
}
pub fn __ink_process_timers() -> Vec<String> { ... }  // returns JS strings
```

**Target:**
```rust
struct TimerEntry {
    func: rquickjs::Function,  // ← Function reference
    ...
}
pub fn process_timers(&self) {  // calls Functions directly
    for timer in due_timers {
        timer.func.call(()).ok();
    }
}
```

### 3. Microtask callbacks (bridge.rs + main.rs)

Same pattern as timers.

## Performance Impact

| Metric | String Callbacks | Function Callbacks |
|--------|-----------------|-------------------|
| Key press latency | ~0.5ms (string+eval) | ~0.05ms (direct call) |
| Timer callback | ~0.3ms (string+eval) | ~0.03ms (direct call) |
| Memory per callback | String (variable) | Function ref (fixed) |
| GC pressure | High (new strings) | Low (reused refs) |

**Result: 10x faster callbacks, essential for 60fps**

## Acceptance Criteria
- [ ] Input registry stores `rquickjs::Function` refs, not strings
- [ ] Timer registry stores `rquickjs::Function` refs, not strings
- [ ] Microtask registry stores `rquickjs::Function` refs, not strings
- [ ] Event loop calls Functions directly (no `ctx.eval` for dispatch)
- [ ] `useInput()` in ink_js.rs stores Function ref in registry
- [ ] `setTimeout/setInterval` in ink_js.rs stores Function ref
- [ ] No `ctx.eval` in hot path of event loop

## Dependencies
- Task 009b (ink_js.rs integration)
- rquickjs Function lifetime management

## SPEC Reference
§7 Performance
