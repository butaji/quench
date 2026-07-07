> **Minimum custom code.** Delegate built-in semantics to proven Rust crates instead of reimplementing them.

# Task 336: Use ecosystem crates for built-ins

## Goal

Adopt mature Rust crates for the heavy lifting inside JavaScript built-ins, limiting custom code to ECMAScript-specific edge cases and error messages.

## Proposed crate mapping

| Built-in area | Crate | Custom layer |
|---------------|-------|--------------|
| BigInt | `num-bigint` | Sign handling, prototype methods |
| Date/Time | `chrono` (already used) | ECMAScript epoch/formatting rules |
| RegExp | `regex` (already in workspace) | JS-syntax adapter, flags, prototype API |
| Ordered objects | `indexmap` | Insertion-order enforcement |
| String interning | `lasso` / `string-interner` | Atom table API |
| Arena allocation | `bumpalo` | AST/HIR node allocation |
| JSON | `serde_json` (already used) | Replacer/reviver, edge cases |

## Acceptance criteria

- [ ] Each built-in delegates to a crate for core semantics where applicable.
- [ ] Custom code only covers spec-mandated deviations (error messages, coercion order, etc.).
- [ ] No new custom arithmetic, date, regex, or JSON parser code is added.

## Targets

- **Suite:** `both`
- **Batch:** 1
- **Target subset:** test262 + TypeScript built-in subsets
- **Blocked by:** 289, 320
- **Exit criteria:** Relevant built-in subsets pass at 100% with no custom reimplementation of crate-covered logic.
