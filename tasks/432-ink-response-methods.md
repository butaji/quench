# Task 432: `ink-response-methods` Example — `Response.json()`, `Response.text()`, `Response.blob()`, `Response.arrayBuffer()`

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 431

## Problem

`Response` instance methods (`json()`, `text()`, `blob()`, `arrayBuffer()`, `ok`, `status`, `statusText`) are fundamental to the Fetch API. Task 307 covers `fetch` but not Response instance methods.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `response.json()`, `response.text()`, etc.
- `Expr::Await` for async response methods
- `Expr::Member` for `response.ok`, `response.status`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-response-methods/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [data, setData] = useState('loading...');

  useEffect(() => {
    const mockResponse = new Response(JSON.stringify({ message: 'hello' }), {
      status: 200,
      statusText: 'OK',
      headers: { 'Content-Type': 'application/json' },
    });

    async function read() {
      const text = await mockResponse.text();
      setData(text);
    }
    read();
  }, []);

  return (
    <Box flexDirection="column">
      <Text>{data}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-response-methods/`
- [ ] Uses `Response` constructor and `.text()` / `.json()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
