# Task 057: TypeScript Examples

## Goal
Create TypeScript versions of all 10 examples to verify TS/TSX → JS transpilation works.

## Status
✅ **10 JavaScript examples exist**  
❌ **0 TypeScript examples exist**

## Required Files

| Example | JS | TS | Task |
|---------|----|----|------|
| Counter | ✅ `counter.js` | ❌ `counter.ts` | 041 |
| Todo List | ✅ `todo-list.js` | ❌ `todo-list.ts` | 042 |
| Focus Form | ✅ `focus-form.js` | ❌ `focus-form.ts` | 043 |
| Dashboard | ✅ `dashboard.js` | ❌ `dashboard.ts` | 044 |
| File Tree | ✅ `file-tree.js` | ❌ `file-tree.ts` | 045 |
| Log Viewer | ✅ `log-viewer.js` | ❌ `log-viewer.ts` | 046 |
| Spinner | ✅ `spinner.js` | ❌ `spinner.ts` | 047 |
| Tabs | ✅ `tabs.js` | ❌ `tabs.ts` | 048 |
| Chat UI | ✅ `chat-ui.js` | ❌ `chat-ui.ts` | 049 |
| Mouse App | ✅ `mouse-app.js` | ❌ `mouse-app.ts` | 050 |

## Pattern

Each `.ts` file should mirror its `.js` counterpart but with:
- TypeScript types
- JSX syntax (if component-based)
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
- [ ] All 10 `.ts` files exist
- [ ] Each transpiles with esbuild without errors
- [ ] Each runs in tuibridge identically to its `.js` version
- [ ] Parity harness verifies ANSI output matches JS version

## Dependencies
- esbuild (dev dependency)
- Tasks 041–050 (JS examples)

## SPEC Reference
§11 Examples
