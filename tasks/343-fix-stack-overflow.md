# Task 310: Fix Stack Overflow in Examples

## Status: CLOSED (duplicate of Task 85)

> The canonical fix for recursive-interpreter stack overflow is Task 85 (trampoline interpreter). This task is closed in favor of that approach.

## Root Cause

The stack overflow occurs because the JavaScript interpreter is recursive:
- Each JavaScript function call creates multiple Rust stack frames
- The hooks/reconciler in runtime.js causes deep call nesting
- The Rust call stack is exhausted before the depth counter catches it

### Call Chain Example (counter.js)
```
render() 
  → mountTree() 
    → ComponentInstance.render()
      → Counter function (calls hooks)
        → mountTree() [recursive for children]
```

### Current Depth Tracking
- `check_depth()` is called at the start of each function call
- Max depth is 10,000
- But each JavaScript call creates ~50-100 Rust stack frames
- So depth of 100 JavaScript calls = 5,000-10,000 Rust stack frames

## Fix Options

### Option 1: Increase Rust Stack Size
Add `RUST_MIN_STACK` environment variable handling or linker flags.

**Pros**: Quick fix
**Cons**: Doesn't scale, platform-dependent

### Option 2: Convert to Iterative Interpreter
Use an explicit call stack instead of Rust recursion.

**Pros**: Solves the problem at the root
**Cons**: Significant refactoring, high effort

### Option 3: Trampoline Pattern
Convert tail-recursive calls to trampoline jumps.

**Pros**: Moderate effort, maintains structure
**Cons**: Only helps tail recursion, not general recursion

### Option 4: Reduce Call Depth Per Frame
Optimize the interpreter to use fewer stack frames per call.

**Pros**: Lower effort than full iterative
**Cons**: May not be enough for deep recursion

## Recommended Approach

**Start with Option 4** - reduce stack frames per call, then move to Option 2 if needed.

### Specific Fixes:

1. **Inline small functions**: Merge `eval_call` logic into `eval_expression` to reduce frame count
2. **Avoid redundant depth checks**: Only check depth for actual function calls, not all expressions
3. **Use tail-call optimization hints**: Mark functions that can be tail-call optimized

## Verification

```bash
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/counter.tsx
timeout 60 cargo run -- examples/animations.tsx
```

## Acceptance Criteria

- [ ] counter.js runs without stack overflow
- [ ] counter.tsx runs without stack overflow  
- [ ] animations.tsx runs without stack overflow
- [ ] All existing tests still pass
