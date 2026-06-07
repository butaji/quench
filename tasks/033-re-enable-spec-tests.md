# Task: Re-enable Disabled Spec Test Modules

**Priority:** P1-High  
**Phase:** 3 — Coverage Gaps  
**Depends on:** 020, 021

## Problem

Currently **11 of 15** test modules in `src/transpile/tests/mod.rs` are commented out with `// #[cfg(test)]` and an `// Ignored: ...` reason comment:

```rust
// #[cfg(test)]
// pub mod completeness_codegen; // Ignored: codegen completeness issues

// #[cfg(test)]
// pub mod parser; // Ignored: parser tests have known issues

// #[cfg(test)]
// pub mod spec_async_runtime; // Ignored: async patterns not fully implemented

// #[cfg(test)]
// pub mod spec_control_flow; // Ignored: control flow patterns not fully implemented

// #[cfg(test)]
// pub mod spec_data_structures; // Ignored: data structure handling not fully implemented

// #[cfg(test)]
// pub mod spec_modules; // Ignored: module handling not fully implemented

// #[cfg(test)]
// pub mod spec_vars_functions; // Ignored: variable and function handling not fully implemented

// #[cfg(test)]
// pub mod spec_roundtrip; // Ignored: roundtrip tests have known issues

// #[cfg(test)]
// pub mod spec_jsx; // Ignored: JSX parsing not implemented

// #[cfg(test)]
// pub mod spec_classes; // Ignored: class support not fully implemented

// #[cfg(test)]
// pub mod spec_stdlib; // Ignored: stdlib tests have known issues
```

Only 4 modules are currently enabled: `analyzer`, `completeness_parser`, `integration`, `rq_parity`.

These disabled modules contain language coverage for:
- **Control flow:** `if`/`else`, `switch`, `for`, `while`, `do-while`, `try`/`catch`, `break`/`continue`, ternary
- **Data structures:** arrays, objects, destructuring, pattern coverage
- **Variables/functions:** variable bindings, arrow functions, async functions, function parameters, destructuring
- **JSX:** elements, attributes, children, fragments, inline styles, event handlers
- **Modules:** imports, exports, default exports, re-exports
- **Classes:** class declarations, methods, inheritance
- **Stdlib:** `Math`, `Date`, `Array`, `String` methods
- **Roundtrip:** parse → HIR → codegen → parse

Without these, there is almost zero automated coverage for the TS/TSX subset that both compile path and Ink examples depend on.

## Steps

1. Review each `// Ignored: ...` comment. If the reason is still valid, keep it disabled but move the reason into the task notes.
2. Uncomment modules whose blockers are resolved (e.g. helper visibility, missing imports).
3. Run `cargo test --bin runts` after each module is re-enabled.
4. For tests that hit `Expr::Invalid` or `Stmt::Empty` (unimplemented parser features), mark them `#[ignore = "parser feature not yet implemented"]` rather than deleting them or disabling the whole module.
5. For tests that panic on `codegen for Invalid expression`, add a guard in quote_codegen to return `None` instead of panicking.
6. Continue until zero modules remain commented out.

## Acceptance Criteria

- [ ] All 11 disabled modules uncommented and compiling.
- [ ] `cargo test --bin runts` passes or has only expected `#[ignore]`d failures.
- [ ] Zero `// #[cfg(test)]` lines remain in `src/transpile/tests/mod.rs`.
- [ ] Every disabled/ignored test has a reason comment.

## Notes

- A previous attempt re-enabled the 4 core spec modules, but the current working tree has them disabled again along with 7 additional modules.
- Task 034 depends on this task: we cannot fix HIR test failures while the modules containing those failures are disabled.
- Target state: 15/15 modules enabled; failures handled via `#[ignore]` with documented reasons, not by commenting out modules.
