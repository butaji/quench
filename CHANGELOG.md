# Changelog

All notable changes to the quench project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Task 65: Documentation cleanup (RUNTIME_STATUS.md, docs/architecture.md)
- Task 63: Architecture split (in progress)

### Changed
- Updated task index with new tasks 63, 64, 65

## [0.1.0] - 2026-07-01

### Added

#### Core Runtime
- **Custom TS/JS/TSX Runtime**: Complete JavaScript runtime built in Rust using `swc` parser
- **ES Modules**: Full `import`/`export` support (named, default, namespace imports/exports)
- **Async/Await**: Complete Promise implementation with microtask draining
- **Classes**: Constructor functions, prototype methods, and inheritance
- **Arrow Functions**: Lexical `this` binding
- **Destructuring**: Object and array destructuring with defaults
- **Rest Parameters**: `...args` in function declarations and expressions
- **Spread Operator**: `...` in arrays, objects, and function calls
- **Template Literals**: String interpolation with `${}` expressions
- **Optional Chaining**: `?.` operator for safe property access
- **Nullish Coalescing**: `??` operator for null/undefined coalescing
- **For...of / For...in**: Iterable iteration with proper protocol support
- **Getters/Setters**: Property accessors with prototype chain walking
- **Switch/break**: Complete with fallthrough handling and labeled breaks
- **Try/Catch**: Error handling with proper exception propagation
- **typeof operator**: Returns string type for all JavaScript values
- **instanceof operator**: Walks prototype chain for object type checking
- **Abstract Equality**: `==` and `!=` with proper type coercion

#### Built-in Objects
- **Array**: Complete `Array.prototype` methods including `map`, `filter`, `reduce`, `flat`, `flatMap`, `find`, `findIndex`, `some`, `every`, `includes`, `indexOf`, `lastIndexOf`, `push`, `pop`, `shift`, `unshift`, `splice`, `slice`, `concat`, `join`, `reverse`, `sort`, `fill`, `copyWithin`
- **Map**: Full implementation with `get`, `set`, `has`, `delete`, `clear`, `forEach`, `size`, insertion order preservation
- **Set**: Full implementation with `add`, `has`, `delete`, `clear`, `forEach`, `size`, insertion order preservation
- **Promise**: `then`, `catch`, `finally`, `all`, `race`, `resolve`, `reject` with proper microtask queue
- **String**: All `String.prototype` methods including `slice`, `substr`, `substring`, `replace`, `split`, `trim`, `toLowerCase`, `toUpperCase`, `charAt`, `charCodeAt`, `codePointAt`, `concat`, `endsWith`, `includes`, `indexOf`, `lastIndexOf`, `localeCompare`, `match`, `matchAll`, `normalize`, `padEnd`, `padStart`, `repeat`, `replaceAll`, `search`, `startsWith`, `toLocaleLowerCase`, `toLocaleUpperCase`, `trimEnd`, `trimStart`
- **Number**: `toFixed`, `toPrecision`, `toExponential`, `toString`, `valueOf`
- **Boolean**: Complete wrapper with prototype methods
- **Object**: `hasOwnProperty`, `toString`, `valueOf`, `is`, `assign`, `create`, `defineProperty`, `defineProperties`, `freeze`, `seal`, `preventExtensions`, `isFrozen`, `isSealed`, `isExtensible`, `getPrototypeOf`, `setPrototypeOf`, `keys`, `values`, `entries`
- **Date**: Full implementation including `now`, `parse`, `UTC`, `toTimeString`, `toLocaleTimeString`, `getFullYear`, `getMonth`, `getDate`, `getDay`, `getHours`, `getMinutes`, `getSeconds`, `getMilliseconds`, `getTime`, `getTimezoneOffset`
- **Error**: `Error`, `TypeError`, `ReferenceError`, `RangeError`, `SyntaxError` with stack traces
- **JSON**: `parse` and `stringify` with proper serialization
- **Math**: All methods including `abs`, `floor`, `ceil`, `round`, `sqrt`, `pow`, `max`, `min`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `log`, `log10`, `log2`, `exp`, `PI`, `E`, `random`
- **console**: `log`, `error`, `warn`, `info`, `debug` with output capture

#### TypeScript Support
- **Type Annotations**: Stripped during lowering
- **Interfaces**: Stripped
- **Type Aliases**: Stripped
- **Enums**: Runtime evaluation support
- **Declare Statements**: Stripped
- **TSX**: Full JSX support with `<Component />` syntax

#### Testing Infrastructure
- **TypeScript Conformance Harness**: Runs TypeScript test suite in quench-runtime
- **Compiler Cases Harness**: Runs ~6500 TypeScript compiler regression cases
- **Evaluation Harness**: Runs TypeScript evaluation unit tests
- **Parity Tests**: Integration tests for Ink examples
- **Unit Tests**: Comprehensive unit tests in `crates/quench-runtime/tests/`

#### CI/CD
- **GitHub Actions**: CI pipeline with sanity, whitelist-quick, and whitelist-full jobs
- **Conformance Thresholds**: 50% minimum pass rate enforced

#### Build System
- **Build Linter**: Enforces 500-line file limit, 40-line function limit, complexity 10
- **Clippy**: All warnings resolved
- **Documentation**: Comprehensive docs for conformance, performance, and TypeScript testing

### Fixed

#### Parser/Lowering (Task 01, 53)
- Computed member access
- Template literal interpolation
- For...of/for...in loops
- Object/array spread
- Rest parameters in declarations
- Nullish coalescing (`??`)
- `in` and `instanceof` operators
- Getters/setters
- Module/script fallback
- Optional chaining
- Destructuring assignments
- Do-while loops
- Tagged template literals
- Export default expressions

#### Interpreter (Task 03, 53, 54, 59, 61)
- Break/continue in loops
- Arrow function `this` binding
- Abstract equality comparison
- Prototype chain walking for `instanceof`
- Stack overflow protection
- Function hoisting with proper rest params
- Function redeclaration handling
- Assignment LHS re-evaluation
- String.prototype.split population

#### Built-ins (Task 02, 04, 13, 14, 54)
- Array.prototype methods returning proper arrays
- Array method chaining (`.map().filter()`)
- String/Number/Boolean prototype linkage
- Set insertion order
- Map/Set for...of iteration
- Object.keys/values/entries for arrays
- Date.now static method
- Date.prototype.toTimeString
- Number.prototype.toFixed
- Math trigonometric functions
- Array.from iterables
- Promise executor invocation

#### Bridge/Host Functions (Task 05, 06)
- Event dispatch to runtime.js handlers
- Microtask draining after event loop
- `__ink_*` function registration
- Typed argument extraction
- JSON serialization
- Native function this binding

### Changed

#### Architecture
- Monolithic files split into subdirectories (partial, Task 63)
- Iterative interpreter with explicit depth tracking
- Thread-local prototype storage
- Recursion guard (MAX_RECURSION_DEPTH = 10000)

#### Performance
- `rustc-hash` for HashMaps
- `indexmap` for ordered property maps
- Benchmarks in `tests/benchmarks.rs`

## [0.0.0] - 2026-06-01

### Added
- Initial project setup
- Basic swc parser integration
- Initial value and object model
- Bridge infrastructure
- Ink runtime integration
- CLI interface

[Unreleased]: https://github.com/earendil-works/quench/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/earendil-works/quench/releases/tag/v0.1.0
[0.0.0]: https://github.com/earendil-works/quench/releases/tag/v0.0.0
