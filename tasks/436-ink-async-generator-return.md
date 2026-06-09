# Task 436: `ink-async-generator-return` Example — Async Generator `return()` and `throw()`

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 435

## Problem

Async generator `return()` and `throw()` methods control async iteration. Task 290 covers regular generator return/throw, and Task 275 covers async generators, but the combination of async generator control methods is not explicitly exercised.

## HIR Coverage

- `Expr::AsyncFunction` / `Expr::Generator` for async generator declarations
- `Expr::Call` for `gen.return()` and `gen.throw()`
- `Expr::Await` for async iteration

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping for async generator control methods

## Ink Example

```tsx
// examples/ink-async-generator-return/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

async function* asyncGen() {
  yield 1;
  yield 2;
  yield 3;
}

async function run(): Promise<string[]> {
  const results: string[] = [];
  const gen = asyncGen();
  const first = await gen.next();
  results.push(`first:${first.value}`);

  const ret = await gen.return!(99);
  results.push(`return:${ret.value}`);

  const after = await gen.next();
  results.push(`after:${after.done}`);

  return results;
}

const result = await run();

export default function App() {
  return (
    <Box flexDirection="column">
      {result.map((line, i) => (
        <Text key={i}>{line}</Text>
      ))}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-async-generator-return/`
- [ ] Uses async generator with `.return()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
