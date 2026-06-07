# runts-ink — Supported TypeScript/TSX Feature Coverage

> **Version:** 1.0.0-draft  
> **Status:** In Progress  
> **Last Updated:** 2026-06-07  
> **Architecture:** rquickjs (dev engine) + Yoga (layout) + Ratatui (render)

---

## Overview

This document maps every TypeScript/TSX/React/Ink feature to its implementation status across the runts-ink pipeline:

1. **Parser** (oxc) — Converts TS/TSX source to HIR
2. **HIR** — High-level Intermediate Representation
3. **Codegen** — Generates compilable Rust
4. **Example** — Ink example exercising the feature
5. **Test** — Automated test coverage

---

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Fully implemented and tested |
| ⚠️ | Partial implementation or known limitations |
| ❌ | Not implemented (compile error) |
| N/A | Not applicable (JS runtime feature) |

---

## 1. Core JavaScript Features

### 1.1 Variables & Binding

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| `const` declaration | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `let` declaration | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `var` declaration | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multiple declarators (`let a=1, b=2`) | ✅ | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| Object destructuring | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Array destructuring | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Destructuring with defaults | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Destructuring with rest | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Nested destructuring | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Renamed destructuring (`{a: b}`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### 1.2 Functions

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Function declaration | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Function expression | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Arrow functions | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Anonymous functions | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Default parameters | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Rest parameters (`...args`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Async functions | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Generator functions (`function*`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `yield` / `yield*` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Method syntax in objects | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### 1.3 Control Flow

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| `if` / `else` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `switch` / `case` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `for` (C-style) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `for...in` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `for...of` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `while` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `do...while` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `break` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `continue` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `return` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `try` / `catch` / `finally` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `throw` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### 1.4 Expressions & Operators

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Arithmetic (`+`, `-`, `*`, `/`, `%`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Comparison (`==`, `===`, `!=`, `!==`, `<`, `<=`, `>`, `>=`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Logical `&&` / `\|\|` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Nullish coalescing `??` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Optional chaining `?.` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Unary (`!`, `-`, `+`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `typeof` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `void` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `delete` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `instanceof` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Ternary (`cond ? a : b`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Comma operator | ✅ | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| Spread in arrays | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Spread in objects | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Spread in function calls | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### 1.5 Compound Assignment Operators

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| `=` (simple assignment) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `+=` / `-=` / `*=` / `/=` / `%=` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `<<=` / `>>=` / `>>>=` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `&=` / `\|=` / `^=` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Logical `&&=` / `\|\|=` / `??=` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### 1.6 Template Literals

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Simple template `` `hello` `` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Template with interpolation | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multi-interpolation | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Nested templates | ✅ | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| Tagged templates | ⚠️ | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |

---

## 2. Object-Oriented Features

### 2.1 Classes

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Class declaration | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Class expression | ✅ | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| Constructor | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Instance methods | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Static methods | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `extends` inheritance | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `super` calls | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Getters | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Setters | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Private fields (`#field`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Public/private/protected modifiers | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| `readonly` modifier | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| Abstract classes | ⚠️ | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| Class decorators | ✅ | ✅ | N/A | ❌ | ✅ | ⚠️ |
| Method decorators | ✅ | ✅ | N/A | ❌ | ✅ | ⚠️ |
| Property decorators | ✅ | ✅ | N/A | ❌ | ✅ | ⚠️ |

### 2.2 Enums

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Numeric enums | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| String enums | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Const enums | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Enum member expressions | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## 3. Module System

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Static imports | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Named imports | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Default imports | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Namespace imports (`* as X`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `export` declarations | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Named exports | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Default exports | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Re-exports (`export from`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Barrel exports (`export *`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `import type` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `export type` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Dynamic `import()` | ✅ | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `import.meta` | ✅ | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `export =` (CommonJS) | ⚠️ | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |

---

## 4. TypeScript Type System

### 4.1 Primitive Types

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| `string` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `number` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `boolean` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `undefined` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `null` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `void` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `any` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `unknown` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `never` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `symbol` | ✅ | ✅ | N/A | ❌ | ❌ | ❌ |
| `bigint` | ✅ | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |

### 4.2 Type Annotations

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| Variable annotations | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Parameter annotations | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Return type annotations | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Property annotations | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |

### 4.3 Type Declarations

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| `type` alias | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `interface` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Interface extends | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Union types | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Intersection types | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Literal types | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Tuple types | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Array types (`T[]`) | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `as` type assertion | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `satisfies` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Non-null assertion (`!`) | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `keyof` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `readonly` arrays | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Index signatures | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| Mapped types | ✅ | ⚠️ | N/A | ✅ | ✅ | ⚠️ |
| Conditional types | ✅ | ⚠️ | N/A | ✅ | ✅ | ⚠️ |
| `infer` | ✅ | ⚠️ | N/A | ✅ | ✅ | ⚠️ |
| Template literal types | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `this` types | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |

### 4.4 Utility Types

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| `Partial<T>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Required<T>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Pick<T, K>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Omit<T, K>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Record<K, V>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `ReturnType<T>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Parameters<T>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `ConstructorParameters<T>` | ✅ | ✅ | N/A | ⚠️ | ⚠️ | ⚠️ |
| `InstanceType<T>` | ✅ | ✅ | N/A | ⚠️ | ⚠️ | ⚠️ |
| `Extract<T, U>` | ✅ | ✅ | N/A | ⚠️ | ⚠️ | ⚠️ |
| `Exclude<T, U>` | ✅ | ✅ | N/A | ⚠️ | ⚠️ | ⚠️ |
| `NonNullable<T>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Readonly<T>` | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `Awaited<T>` | ✅ | ✅ | N/A | ⚠️ | ⚠️ | ⚠️ |

---

## 5. JSX / TSX

| Feature | Parser | HIR | Codegen | Example | Tests | Status |
|---------|--------|-----|---------|---------|-------|--------|
| HTML elements | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Self-closing elements | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Component elements | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Fragments (`<>...</>`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Children | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Text expressions | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Conditional rendering | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| List rendering | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Spread props | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Boolean attributes | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Style objects | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Dynamic attributes | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Event handlers | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |
| `key` prop | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `ref` prop | ✅ | ✅ | N/A | ✅ | ✅ | ✅ |

---

## 6. React Hooks (rquickjs)

| Feature | Implementation | Example | Tests | Status |
|---------|----------------|---------|-------|--------|
| `useState` | ✅ | ✅ | ✅ | ✅ |
| `useEffect` | ✅ | ✅ | ✅ | ✅ |
| `useLayoutEffect` | ✅ | ✅ | ✅ | ✅ |
| `useRef` | ✅ | ✅ | ✅ | ✅ |
| `useMemo` | ✅ | ✅ | ✅ | ✅ |
| `useCallback` | ✅ | ✅ | ✅ | ✅ |
| `useReducer` | ✅ | ✅ | ✅ | ✅ |
| `useContext` | ✅ | ✅ | ✅ | ✅ |
| `useId` | ✅ | ✅ | ✅ | ✅ |
| `useTransition` | ✅ | ✅ | ✅ | ✅ |
| `useDeferredValue` | ✅ | ✅ | ✅ | ✅ |
| `useSyncExternalStore` | ✅ | ✅ | ✅ | ✅ |
| `useImperativeHandle` | ✅ | ✅ | ✅ | ✅ |
| `useInsertionEffect` | ✅ | ✅ | ✅ | ✅ |
| `useDebugValue` | ✅ | ✅ | ✅ | ✅ |
| `forwardRef` | ✅ | ✅ | ✅ | ✅ |
| `memo` | ✅ | ✅ | ✅ | ✅ |
| `lazy` | ✅ | ✅ | ✅ | ✅ |
| `Suspense` | ✅ | ✅ | ✅ | ✅ |
| `Children` API | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| `cloneElement` | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| `isValidElement` | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

---

## 7. Ink Components

| Feature | Bridge | Example | Tests | Status |
|---------|--------|---------|-------|--------|
| `<Box>` | ✅ | ✅ | ✅ | ✅ |
| `<Text>` | ✅ | ✅ | ✅ | ✅ |
| `<Newline>` | ✅ | ✅ | ✅ | ✅ |
| `<Spacer>` | ✅ | ✅ | ✅ | ✅ |
| `<Static>` | ✅ | ✅ | ✅ | ✅ |
| `<Transform>` | ✅ | ✅ | ✅ | ✅ |
| `<Color>` | ✅ | ✅ | ✅ | ✅ |
| `<Cursor>` | ✅ | ✅ | ✅ | ✅ |
| `<Link>` | ✅ | ✅ | ✅ | ✅ |
| `<ProgressBar>` | ✅ | ✅ | ✅ | ✅ |
| `<Bar>` | ✅ | ✅ | ✅ | ✅ |
| `<Border>` | ✅ | ✅ | ✅ | ✅ |
| `<Table>` | ✅ | ✅ | ✅ | ✅ |
| `<Expander>` | ✅ | ✅ | ✅ | ✅ |
| `<Separator>` | ✅ | ✅ | ✅ | ✅ |
| `<Form>` | ✅ | ✅ | ✅ | ✅ |
| `<Input>` | ✅ | ✅ | ✅ | ✅ |
| `<Select>` | ✅ | ✅ | ✅ | ✅ |
| `<Switch>` | ✅ | ✅ | ✅ | ✅ |
| `ErrorBoundary` | ✅ | ✅ | ✅ | ✅ |

---

## 8. Ink Hooks

| Feature | Bridge | Example | Tests | Status |
|---------|--------|---------|-------|--------|
| `useInput` | ✅ | ✅ | ✅ | ✅ |
| `useApp` | ✅ | ✅ | ✅ | ✅ |
| `useStdin` | ✅ | ✅ | ✅ | ✅ |
| `useStdout` | ✅ | ✅ | ✅ | ✅ |
| `useStderr` | ✅ | ✅ | ✅ | ✅ |
| `useWindowSize` | ✅ | ✅ | ✅ | ✅ |
| `useFocus` | ✅ | ✅ | ✅ | ✅ |
| `useFocusManager` | ✅ | ✅ | ✅ | ✅ |
| `useCursor` | ✅ | ✅ | ✅ | ✅ |
| `useAnimation` | ✅ | ✅ | ✅ | ✅ |
| `useBoxMetrics` | ✅ | ✅ | ✅ | ✅ |
| `measureElement` | ✅ | ✅ | ✅ | ✅ |
| `usePaste` | ✅ | ✅ | ✅ | ✅ |
| `useRawMode` | ✅ | ✅ | ✅ | ✅ |
| `useMouse` | ✅ | ✅ | ✅ | ✅ |

---

## 9. Ink Layout Props

| Feature | Bridge | Example | Tests | Status |
|---------|--------|---------|-------|--------|
| `width` / `height` | ✅ | ✅ | ✅ | ✅ |
| `minWidth` / `minHeight` | ✅ | ✅ | ✅ | ✅ |
| `maxWidth` / `maxHeight` | ✅ | ✅ | ✅ | ✅ |
| `flexDirection` | ✅ | ✅ | ✅ | ✅ |
| `flexWrap` | ✅ | ✅ | ✅ | ✅ |
| `flexGrow` | ✅ | ✅ | ✅ | ✅ |
| `flexShrink` | ✅ | ✅ | ✅ | ✅ |
| `flexBasis` | ✅ | ✅ | ✅ | ✅ |
| `alignItems` | ✅ | ✅ | ✅ | ✅ |
| `alignSelf` | ✅ | ✅ | ✅ | ✅ |
| `alignContent` | ✅ | ✅ | ✅ | ✅ |
| `justifyContent` | ✅ | ✅ | ✅ | ✅ |
| `justifyItems` | ✅ | ✅ | ✅ | ✅ |
| `justifySelf` | ✅ | ✅ | ✅ | ✅ |
| `gap` | ✅ | ✅ | ✅ | ✅ |
| `rowGap` / `columnGap` | ✅ | ✅ | ✅ | ✅ |
| `padding` | ✅ | ✅ | ✅ | ✅ |
| `margin` | ✅ | ✅ | ✅ | ✅ |
| `border` | ✅ | ✅ | ✅ | ✅ |
| `position` | ✅ | ✅ | ✅ | ✅ |
| `top` / `right` / `bottom` / `left` | ✅ | ✅ | ✅ | ✅ |
| `display` | ✅ | ✅ | ✅ | ✅ |
| `zIndex` | ✅ | ✅ | ✅ | ✅ |
| `overflow` | ✅ | ✅ | ✅ | ✅ |
| `position` | ✅ | ✅ | ✅ | ✅ |

---

## 10. Standard Library

### 10.1 Built-in Objects

| Feature | Dev | Compile | Example | Tests | Status |
|---------|-----|---------|---------|-------|--------|
| `Math` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `Date` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `JSON` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `Array` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `String` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `Number` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `Boolean` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `Object` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `Map` | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `Set` | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `WeakMap` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| `WeakSet` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| `Symbol` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| `Proxy` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| `WeakRef` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| `FinalizationRegistry` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |
| `BigInt` | ⚠️ | ⚠️ | ❌ | ❌ | ❌ |

### 10.2 Promise / Async

| Feature | Dev | Compile | Example | Tests | Status |
|---------|-----|---------|---------|-------|--------|
| `Promise` constructor | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `.then()` / `.catch()` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `Promise.resolve/reject` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `Promise.all` | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| `Promise.allSettled` | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `Promise.race` | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `Promise.any` | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `Promise.withResolvers` | ✅ | ⚠️ | ❌ | ❌ | ⚠️ |
| `async` / `await` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `for await...of` | ⚠️ | ⚠️ | ❌ | ❌ | ⚠️ |

---

## 11. HIR Completeness

### 11.1 Expression Variants (38 total)

| Variant | Parser | HIR | Codegen | Status |
|---------|--------|-----|---------|--------|
| `Literal` | ✅ | ✅ | ✅ | ✅ |
| `Identifier` | ✅ | ✅ | ✅ | ✅ |
| `StaticMember` | ✅ | ✅ | ✅ | ✅ |
| `ComputedMember` | ✅ | ✅ | ✅ | ✅ |
| `Call` | ✅ | ✅ | ✅ | ✅ |
| `New` | ✅ | ✅ | ✅ | ✅ |
| `Binary` | ✅ | ✅ | ✅ | ✅ |
| `Unary` | ✅ | ✅ | ✅ | ✅ |
| `Logical` | ✅ | ✅ | ✅ | ✅ |
| `Await` | ✅ | ✅ | ✅ | ✅ |
| `Yield` | ✅ | ✅ | ✅ | ✅ |
| `Conditional` | ✅ | ✅ | ✅ | ✅ |
| `Function` | ✅ | ✅ | ✅ | ✅ |
| `Arrow` | ✅ | ✅ | ✅ | ✅ |
| `Array` | ✅ | ✅ | ✅ | ✅ |
| `Object` | ✅ | ✅ | ✅ | ✅ |
| `Spread` | ✅ | ✅ | ✅ | ✅ |
| `Assignment` | ✅ | ✅ | ✅ | ✅ |
| `Update` | ✅ | ✅ | ✅ | ✅ |
| `This` | ✅ | ✅ | ✅ | ✅ |
| `Super` | ✅ | ✅ | ✅ | ✅ |
| `NewTarget` | ✅ | ✅ | ✅ | ✅ |
| `OptionalCall` | ✅ | ✅ | ✅ | ✅ |
| `OptionalMember` | ✅ | ✅ | ✅ | ✅ |
| `TypeCast` | ✅ | ✅ | N/A | ✅ |
| `TemplateLiteral` | ✅ | ✅ | ✅ | ✅ |
| `TaggedTemplate` | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| `AwaitOptChain` | ✅ | ✅ | ✅ | ✅ |
| `Bind` | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| `Import` | ✅ | ✅ | ⚠️ | ⚠️ |
| `SuperCall` | ✅ | ✅ | ✅ | ✅ |
| `RegExp` | ✅ | ✅ | ✅ | ✅ |
| `BigIntLiteral` | ✅ | ✅ | ⚠️ | ⚠️ |
| `Arguments` | ✅ | ✅ | ✅ | ✅ |
| `Sequence` | ✅ | ✅ | ✅ | ✅ |
| `Class` | ✅ | ✅ | ✅ | ✅ |
| `Decorator` | ✅ | ✅ | N/A | ✅ |
| `Chain` | ✅ | ✅ | ✅ | ✅ |
| `Shadow` | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

### 11.2 Statement Variants (24 total)

| Variant | Parser | Hmt | Codegen | Status |
|---------|--------|-----|---------|--------|
| `Empty` | ✅ | ✅ | ✅ | ✅ |
| `Block` | ✅ | ✅ | ✅ | ✅ |
| `Variable` | ✅ | ✅ | ✅ | ✅ |
| `FunctionDecl` | ✅ | ✅ | ✅ | ✅ |
| `If` | ✅ | ✅ | ✅ | ✅ |
| `DoWhile` | ✅ | ✅ | ✅ | ✅ |
| `While` | ✅ | ✅ | ✅ | ✅ |
| `ForLoop` | ✅ | ✅ | ✅ | ✅ |
| `ForIn` | ✅ | ✅ | ✅ | ✅ |
| `ForOf` | ✅ | ✅ | ✅ | ✅ |
| `Switch` | ✅ | ✅ | ✅ | ✅ |
| `TryCatch` | ✅ | ✅ | ✅ | ✅ |
| `Throw` | ✅ | ✅ | ✅ | ✅ |
| `Return` | ✅ | ✅ | ✅ | ✅ |
| `Break` | ✅ | ✅ | ✅ | ✅ |
| `Continue` | ✅ | ✅ | ✅ | ✅ |
| `Expression` | ✅ | ✅ | ✅ | ✅ |
| `Class` | ✅ | ✅ | ✅ | ✅ |
| `ExportNamed` | ✅ | ✅ | ✅ | ✅ |
| `ExportDefault` | ✅ | ✅ | ✅ | ✅ |
| `ExportAll` | ✅ | ✅ | ✅ | ✅ |
| `Import` | ✅ | ✅ | ✅ | ✅ |
| `Labeled` | ✅ | ✅ | ✅ | ✅ |
| `With` | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| `Enum` | ✅ | ✅ | ✅ | ✅ |
| `Interface` | ✅ | ✅ | N/A | ✅ |
| `TypeAlias` | ✅ | ✅ | N/A | ✅ |

---

## 12. Current Statistics

| Metric | Value |
|--------|-------|
| Examples | 127 |
| Examples with tests | 120 (94.5%) |
| Tests passing | 987 |
| Tests ignored | 180 |
| HIR expr variants | 38 |
| HIR expr codegen | 30 (79%) |
| HIR stmt variants | 24 |
| HIR stmt codegen | 16 (67%) |
| Compile path coverage | ~70% |

---

## 13. Known Gaps

### 13.1 Missing Examples (P2)

- [ ] `ink-bigint-globalthis` — BigInt, numeric separators, globalThis
- [ ] `ink-symbol-collections` — Symbol, Map, Set, WeakMap
- [ ] `ink-suspense-lazy` — Suspense, lazy
- [ ] `ink-error-boundary` — ErrorBoundary (partially in ink-react-advanced)
- [ ] `ink-namespace-declare` — namespace, declare
- [ ] `ink-override-implements` — override, implements
- [ ] `ink-abstract-class` — abstract classes
- [ ] `ink-new-target` — new.target
- [ ] `ink-reflect-api` — Reflect API
- [ ] `ink-template-literal-types` — template literal types
- [ ] `ink-infer-conditional` — infer in conditional types
- [ ] `ink-regexp-advanced` — RegExp flags, matchAll

### 13.2 Missing Features (P3)

- [ ] `WeakRef` / `FinalizationRegistry`
- [ ] `Symbol`
- [ ] `Proxy`
- [ ] Tagged templates
- [ ] Abstract classes
- [ ] `with` statement
- [ ] `eval()` / `new Function()`
- [ ] Dynamic `import.meta`

---

*Last updated: 2026-06-07*
