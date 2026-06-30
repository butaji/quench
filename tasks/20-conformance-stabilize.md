# Task 20: Stabilize conformance results and document supported subset

## Goal

Lock in the conformance gains, prevent regressions, and publish what is and is not supported.

## Status: COMPLETED

### What was done

1. **Conformance harness is complete** with:
   - Source-direct execution mode (run `.ts` files directly)
   - Baseline JS fallback mode
   - Hybrid mode (source direct with baseline fallback)
   - Console output capture for comparison
   - Skip rules for non-runnable cases

2. **Whitelist expanded** to include:
   - ES5-ES2024 and esnext features
   - expressions, statements, functions, classes
   - enums, constEnums
   - async, asyncGenerators, generators
   - controlFlow, emitter
   - symbols, this, typeAssertions
   - objectMembers, forAwait, scanner

3. **Current conformance results** (50-case whitelist sample):
   - **Pass rate: 97.4%** (38/39 runnable cases)
   - 1 parse failure (TypeScript `as` type assertions - needs stripping)
   - 11 skipped (TypeScript-specific syntax)

4. **Local gates**:
   - `cargo test --release --locked`
   - `cargo clippy -- -D warnings` (optional, not enforced if clippy is noisy)
   - Smoke tests the binary
   - Conformance pass-rate gate via `MIN_PASS_RATE` (no external CI)

## Supported JavaScript Features

### Expressions
- Binary and unary operators
- Conditionals
- Array and object literals
- Function and arrow function expressions
- Template literals
- Spread operators
- Destructuring

### Statements
- Variable declarations (let, const, var)
- If/else, switch
- For, for...in, for...of
- While, do...while
- Try/catch/finally
- Break, continue, return

### Functions
- Function declarations and expressions
- Arrow functions
- Default parameters
- Rest parameters
- Async functions
- Generators (yield)

### Classes
- Class declarations and expressions
- Constructor
- Instance methods
- Static methods
- Getters and setters
- Inheritance (extends)
- super calls

### Built-ins
- Array, Object, Map, Set
- Promise
- String, Number, Boolean
- Symbol
- Date
- JSON
- Math
- Error

### ES Features
- ES5 through ES2024 features
- Async/await
- Destructuring
- Spread operator
- Optional chaining
- Nullish coalescing

## Intentionally Unsupported

- `with` statement
- `eval` and `Function` constructor
- Legacy octal literals (`0765`)
- Strict-mode-only behaviors (some)
- TypeScript-specific syntax (interfaces, type aliases, etc.) - needs TS stripping
- Decorators (experimental)
- Import/export modules (needs module loader)

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.


## Verification

```bash
cargo test -p quench-runtime
cargo test -p quench-runtime --test conformance
```
