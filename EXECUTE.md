Spec: rune_spec_v1.md -- readonly! echa task == commit, dont push.

Build a Rust compiler driver named rune that makes *.r.ts and *.r.tsx valid source files for Rust projects with zero runtime overhead. Implement as a cargo subcommand (cargo-rune). Use latest stable rust version.

Code hygiene: with linter make sure that: max file size 500 lines, max function size 40 lines, max function complexity 10. 

Tests coverage, we need comprehensive tests coverage to make sure everything is working as expected.

Examples: have a comprehensive set of examples (/examples) with the most popular scenarios of usage (using Rust standard libraries, Ratatui, and most popular libraries). Show usage of TypeScript and TSX subsets implementing those scenarios. Validate all of them.

Eventually make code and arch review, fix critical and major issues, as well quick minors.

Core behavior. cargo rune dev scans the workspace for *.r.ts and *.r.tsx, parses them with SWC into valid TS AST, validates the zero-overhead subset (no any, no class, no try/catch, no dynamic property access, no var, no loose ==), transpiles to Rust source into target/rune-cache/ (never in the source tree), and compiles the target crate as a cdylib for hot reload. cargo rune build --release transpiles inline and produces a single static binary indistinguishable from hand-written Rust.

Type mapping. number → f64 by default, integer literals → i32, bitwise contexts → i32, array indices → usize. string → String (heap), string literals → &str borrow. boolean → bool. T | null → Option<T>. Tagged unions with mandatory tag field → Rust enums with exhaustive match. Result<T,E> pattern via {ok, value} / {ok, error} objects, recognized and emitted with the ? operator.

Ownership inference. The analyzer infers &T, &mut T, or owned T from usage patterns. const bindings → immutable, let → mutable. Move semantics enforced: using a value after passing to a consuming function is a compile error. .clone() is the explicit escape hatch. Closures capture by reference; mutable captures emit FnMut.

Native interop. import { foo } from "native:bar" in Rune resolves to crate::native::bar::foo in the same crate — zero FFI, same compilation unit. Hand-written .rs files in native/ coexist with .r.ts files in the same crate.

JSX/TSX. Standard JSX syntax transpiles to Rust builder-pattern widget construction (e.g., Ratatui). JSX expressions map to function calls returning impl Widget.

Hot reload. Development builds a cdylib in target/rune-cache/. The host binary (thin, ~80 lines, state owner) loads the dylib via libloading. On file change, cargo rune dev rebuilds the dylib, writes a versioned copy to target/hot/libapp_<timestamp>.so (or .dylib/.dll), atomically updates a target/hot/.current symlink. The host polls .current, unloads the old dylib, loads the new one. AppState lives in the host heap and survives swaps. If protocol/ (shared state trait) changes, trigger full restart with serde serialization roundtrip of AppState.

Error translation. Map rustc borrow checker and type errors back to .r.ts line numbers. Display JS variable names in diagnostics. Emit warnings for integer division inference (5 / 2 → i32 division, not f64).

Code quality requirements for the driver itself. Enforce strict Rust standards: max 500 lines per file, max 40 lines per function, cyclomatic complexity ≤ 10 (Clippy). Use cargo clippy with -D warnings. The driver must be split into clean modules: parser (SWC integration), analyzer (subset validation + borrow inference), codegen (TS AST → Rust source), driver (orchestration + cargo integration), reload (dylib watcher + host signaler).

Deliverables. Working cargo-rune CLI with dev, build, check, and transpile subcommands. A sample project (examples/todox/) demonstrating .r.ts logic, .r.tsx Ratatui UI, and .rs native math functions coexisting in one hot-reloadable crate. All generated code stays in target/. No .generated/ folders in source trees.