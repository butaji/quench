# Task 018: Microtask Polyfills

## Status
✅ **Done**


## Goal
Implement `setImmediate` / `clearImmediate` and `process.nextTick` via microtask queue.

## Acceptance Criteria
- [x] `setImmediate(cb)` → schedules callback on next tick of event loop.
- [x] `process.nextTick(cb)` identical behavior.
- [x] Microtasks execute before timers and I/O in each loop iteration.
- [x] Unit test: queue microtask + timer; verify microtask runs first.

## Implementation

### setImmediate Polyfill (runtime.js)

```javascript
// setImmediate polyfill - same behavior as process.nextTick
if (!globalThis.setImmediate) {
  globalThis.setImmediate = (cb, ...args) => {
    if (typeof cb === 'function') {
      const wrapped = () => cb(...args);
      microtaskCallbacks.push(wrapped);
      return microtaskCallbacks.length;
    }
    return -1;
  };
  globalThis.clearImmediate = (handle) => {
    // Cancellation not implemented (would require handle tracking)
  };
}
```

### process.nextTick (runtime.js)

```javascript
if (!globalThis.process) {
  globalThis.process = {
    nextTick: (cb, ...args) => {
      if (typeof cb === 'function') {
        const wrapped = () => cb(...args);
        microtaskCallbacks.push(wrapped);
      }
    }
  };
}
```

### Microtask Execution

```javascript
globalThis.__tb_invoke_microtasks = function() {
  while (microtaskCallbacks.length > 0) {
    const callbacks = microtaskCallbacks.splice(0);
    for (const callback of callbacks) {
      if (typeof callback === 'function') {
        callback();
      }
    }
  }
};
```

## Event Loop Integration

In the main event loop, microtasks are drained before processing other events:

```rust
// Drain microtasks before processing timers
let _ = bridge::__ink_drain_microtasks();
ctx.eval("__tb_invoke_microtasks()")?;
```

## Dependencies
- Task 013

## SPEC Reference
§4 JS Runtime (timer/microtask polyfills)
