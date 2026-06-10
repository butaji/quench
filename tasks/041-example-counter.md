# Task 041: Example: Counter (JS + TS)

## Goal
Counter app with useState, useEffect, useInput demonstrating stateful re-rendering.

## Status
✅ **JS version works** — `examples/counter.js` runs with full stateful re-rendering via runtime.js reconciler.

## Acceptance Criteria
- [x] `examples/counter.js` — Counter in JavaScript with useState
- [x] `examples/counter.tsx` — Counter in TypeScript/TSX (same logic)
- [x] Both produce identical ANSI output
- [x] Parity harness verifies 100% match

## Working Example (JavaScript)

```javascript
// examples/counter.js
var useState = ink.useState;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;
var Box = ink.Box;
var Text = ink.Text;

function Counter() {
  var _useState = useState(0),
      count = _useState[0],
      setCount = _useState[1];

  useInput(function(input, key) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === ' ') {
      setCount(function(c) { return c + 1; });
    }
  });

  useEffect(function() {
    var timer = setInterval(function() {
      setCount(function(c) { return c + 1; });
    }, 1000);
    return function() { clearInterval(timer); };
  }, []);

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { color: 'green', bold: true, children: 'Counter App' } },
        { type: Text, props: { children: 'Count: ' + count } },
        { type: Text, props: { dimColor: true, children: '[space] increment | [q] quit' } },
      ]
    }
  };
}

render({ type: Counter, props: {} });
```

### Run
```bash
tuibridge examples/counter.js
```

**Features demonstrated:**
- `useState` with functional updater `setCount(c => c + 1)`
- `useInput` for keyboard handling
- `useEffect` with cleanup (interval + return function)
- `setInterval` timer polyfill
- Component re-rendering on state change
- `Box` with borders, `Text` with colors

## TypeScript/TSX Version

```tsx
// examples/counter.tsx
import { render, Box, Text, useState, useInput, useApp, useEffect } from 'ink';

function Counter() {
  const [count, setCount] = useState(0);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === ' ') setCount((c: number) => c + 1);
  });

  useEffect(() => {
    const timer = setInterval(() => setCount(c => c + 1), 1000);
    return () => clearInterval(timer);
  }, []);

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text color="green" bold>Counter App</Text>
      <Text>Count: {count}</Text>
      <Text dimColor>[space] increment | [q] quit</Text>
    </Box>
  );
}

render(<Counter />);
```

### Build and Run
```bash
# With --features compiler:
tuibridge --run examples/counter.tsx

# Or compile first:
tuibridge --compile examples/counter.tsx -o examples/counter.js
tuibridge examples/counter.js
```

## Dependencies
- Task 009b (ink_js.rs integration)
- Task 012 (hooks — via runtime.js reconciler)
- Task 025 (Box render)
- Task 026 (Text render)

## SPEC Reference
§9 Example
