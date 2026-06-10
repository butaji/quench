# Task 057: TypeScript Examples

## Status
✅ **Complete** — All 10 TypeScript examples exist and mirror their JS counterparts.

## Files

| Example | JS | TS | Status |
|---------|----|----|--------|
| Counter | ✅ `counter.js` | ✅ `counter.ts` | Done |
| Todo List | ✅ `todo-list.js` | ✅ `todo-list.ts` | Done |
| Focus Form | ✅ `focus-form.js` | ✅ `focus-form.ts` | Done |
| Dashboard | ✅ `dashboard.js` | ✅ `dashboard.ts` | Done |
| File Tree | ✅ `file-tree.js` | ✅ `file-tree.ts` | Done |
| Log Viewer | ✅ `log-viewer.js` | ✅ `log-viewer.ts` | Done |
| Spinner | ✅ `spinner.js` | ✅ `spinner.ts` | Done |
| Tabs | ✅ `tabs.js` | ✅ `tabs.ts` | Done |
| Chat UI | ✅ `chat-ui.js` | ✅ `chat-ui.ts` | Done |
| Mouse App | ✅ `mouse-app.js` | ✅ `mouse-app.ts` | Done |

## Pattern

Each `.ts` file mirrors its `.js` counterpart with:
- TypeScript types
- JSX syntax (component-based)
- `import` statements instead of `var`

### Example: counter.ts

```typescript
import { render, Box, Text, useState, useInput, useApp, useEffect } from 'ink';

interface CounterProps {}

function Counter(_props: CounterProps): JSX.Element {
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
# Transpile
npx esbuild examples/counter.ts --bundle --outfile=dist/counter.js \
  --external:ink --jsx-factory=createElement --jsx-fragment=Fragment

# Run
tuibridge dist/counter.js
```

## Acceptance Criteria
- [x] All 10 `.ts` files exist
- [x] Each transpiles with esbuild without errors
- [x] Each runs in tuibridge identically to its `.js` version
- [x] Parity harness verifies ANSI output matches JS version

## Dependencies
- esbuild (dev dependency)
- Tasks 041–050 (JS examples)

## SPEC Reference
§10 Examples Matrix
