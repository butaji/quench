# Task 065: Compile React+Ink Apps for Quench (esbuild approach — deprecated)

## Goal
Compile React+Ink source files (like `../tui1/mod.tsx`) into Quench-compatible JS — without running pre-built bundles.

## Problem

`tui1/mod.tsx` uses React imports:
```tsx
import React, { useState, useEffect, useRef } from "react";
import { render, Text, Box } from "ink";
```

Quench expects globals:
```js
const { useState, useEffect, render, Box, Text } = ink;
```

We need a build step that transforms one into the other.

## Simple Solution: esbuild Transform

Use esbuild with aliases and a custom JSX transform to compile React+Ink → Quench.

### Transform Rules

| Source | Target |
|--------|--------|
| `import React, { useState } from "react"` | `const { useState } = ink;` |
| `import { render, Box, Text } from "ink"` | `const { render, Box, Text } = ink;` |
| `React.useState` | `ink.useState` |
| JSX `<Box>` | `createElement(Box, ...)` |
| `process.exit(0)` | `ink.useApp().exit()` |

### Build Command

```bash
npx esbuild ../tui1/mod.tsx \
  --bundle \
  --platform=neutral \
  --format=iife \
  --global-name=App \
  --jsx-factory=createElement \
  --jsx-fragment=Fragment \
  --define:process.env.NODE_ENV='"production"' \
  --inject:scripts/react-to-ink-shim.js \
  --outfile=dist/tui1-for-quench.js
```

### Shim File (`scripts/react-to-ink-shim.js`)

```js
// Injected at top of bundle — provides React/Ink API from Quench globals
const React = {
  useState: ink.useState,
  useEffect: ink.useEffect,
  useRef: ink.useRef,
  useMemo: ink.useMemo,
  useCallback: ink.useCallback,
  useContext: ink.useContext,
  createElement: ink.createElement,
  Fragment: ink.Fragment || (({ children }) => children),
};

const render = ink.render;
const Box = ink.Box;
const Text = ink.Text;
const Static = ink.Static;
const Newline = ink.Newline;
const Spacer = ink.Spacer;
const useInput = ink.useInput;
const useApp = ink.useApp;
const useStdin = ink.useStdin;
const useStdout = ink.useStdout;
const useStderr = ink.useStderr;
const useFocus = ink.useFocus;
const useFocusManager = ink.useFocusManager;
const measureElement = ink.measureElement;
const createContext = ink.createContext;

// process polyfill
const process = {
  exit: (code) => ink.useApp().exit(),
  stdout: { write: (s) => ink.stdout_write(s) },
  stderr: { write: (s) => ink.stderr_write(s) },
  env: {},
};
```

### What Gets Stripped

esbuild `--platform=neutral` with `--external:react` and `--external:ink` would fail because the shim needs to replace them. Instead:

```bash
# Better: mark react and ink as external, then inject shim
npx esbuild ../tui1/mod.tsx \
  --bundle \
  --platform=neutral \
  --format=iife \
  --external:react \
  --external:ink \
  --jsx-factory=createElement \
  --jsx-fragment=Fragment \
  --inject:scripts/react-to-ink-shim.js \
  --outfile=dist/tui1-for-quench.js
```

But `--external` leaves `import` statements, which QuickJS doesn't handle well. Better approach:

### Plugin Approach (esbuild plugin)

```js
// scripts/quench-build-plugin.js
const quenchPlugin = {
  name: 'quench',
  setup(build) {
    // Replace "react" imports with shim
    build.onResolve({ filter: /^react$/ }, () => ({
      path: 'react',
      namespace: 'quench-shim',
    }));
    build.onLoad({ filter: /.*/, namespace: 'quench-shim' }, () => ({
      contents: `export default React; export const { useState, useEffect, useRef, useMemo, useCallback, useContext, createElement, Fragment } = React;`,
      loader: 'js',
    }));

    // Replace "ink" imports with shim
    build.onResolve({ filter: /^ink$/ }, () => ({
      path: 'ink',
      namespace: 'quench-shim',
    }));
    build.onLoad({ filter: /.*/, namespace: 'quench-shim-ink' }, () => ({
      contents: `export { render, Box, Text, Static, Newline, Spacer, useInput, useApp, useStdin, useStdout, useStderr, useFocus, useFocusManager, measureElement, createContext };`,
      loader: 'js',
    }));
  },
};
```

### Even Simpler: Wrapper Script

```bash
#!/bin/bash
# scripts/build-for-quench.sh — compile any React+Ink app for Quench

INPUT="$1"
OUTPUT="${2:-dist/quench-app.js}"

# Create temp shim
cat > /tmp/tb-shim.js << 'SHIM'
const React = {
  useState: ink.useState, useEffect: ink.useEffect, useRef: ink.useRef,
  useMemo: ink.useMemo, useCallback: ink.useCallback, useContext: ink.useContext,
  createElement: ink.createElement, Fragment: ink.Fragment || (({children}) => children),
};
const render = ink.render, Box = ink.Box, Text = ink.Text, Static = ink.Static;
const Newline = ink.Newline, Spacer = ink.Spacer, useInput = ink.useInput;
const useApp = ink.useApp, useStdin = ink.useStdin, useStdout = ink.useStdout;
const useStderr = ink.useStderr, useFocus = ink.useFocus;
const useFocusManager = ink.useFocusManager, measureElement = ink.measureElement;
const createContext = ink.createContext;
const process = {
  exit: () => ink.useApp().exit(), stdout: {write: (s) => ink.stdout_write(s)},
  stderr: {write: (s) => ink.stderr_write(s)}, env: {}
};
SHIM

# Bundle with esbuild
npx esbuild "$INPUT" \
  --bundle \
  --platform=neutral \
  --format=iife \
  --global-name=__app \
  --inject:/tmp/tb-shim.js \
  --jsx-factory=createElement \
  --jsx-fragment=Fragment \
  --define:process.env.NODE_ENV='"production"' \
  --outfile="$OUTPUT"

echo "Built: $OUTPUT"
echo "Run: quench $OUTPUT"
```

## Usage

```bash
# Build tui1 for Quench
./scripts/build-for-quench.sh ../tui1/mod.tsx ../tui1/dist/tui1-tb.js

# Run
quench ../tui1/dist/tui1-tb.js
```

## Gap Analysis: tui1/mod.tsx

What tui1 uses vs what Quench provides:

| tui1 Usage | Quench Status | Action |
|-----------|------------------|--------|
| `useState` | ✅ | `ink.useState` |
| `useEffect` | ✅ | `ink.useEffect` |
| `useRef` | ✅ | `ink.useRef` |
| `render` | ✅ | `ink.render` |
| `Box` | ✅ | `ink.Box` |
| `Text` | ✅ | `ink.Text` |
| Custom colors (`#0c0c0c`) | ✅ | Hex colors supported |
| `process.exit(0)` | ⚠️ | Polyfill via `ink.useApp().exit()` |
| Keyboard shortcuts (ctrl+n) | ⚠️ | `useInput` captures single keys; combos need mapping |

## Minimal Example

```bash
# Test with a simple React-style file
cat > /tmp/test-react.tsx << 'EOF'
import React, { useState } from "react";
import { render, Box, Text } from "ink";

function Counter() {
  const [count, setCount] = useState(0);
  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text color="green" bold>React → Quench</Text>
      <Text>Count: {count}</Text>
    </Box>
  );
}

render(<Counter />);
EOF

./scripts/build-for-quench.sh /tmp/test-react.tsx /tmp/test-tb.js
quench /tmp/test-tb.js
```

## Files to Create

- `scripts/build-for-quench.sh` — Build script
- `scripts/react-to-ink-shim.js` — Shim for esbuild inject

## Modified Files

- `docs/SPEC.md` — Document React app compilation

## Acceptance Criteria
- [ ] `build-for-quench.sh` compiles `../tui1/mod.tsx` without errors
- [ ] Compiled app runs under `quench` in tmux
- [ ] Visual output matches Deno/Ink version
- [ ] Keyboard input works (arrow keys, space, q)

## Status
**Pending** — Not started

## Dependencies
- None (builds on existing `runtime.js` hooks)
