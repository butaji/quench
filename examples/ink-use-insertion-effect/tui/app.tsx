// ink-use-insertion-effect example — demonstrates useInsertionEffect hook.
//
// useInsertionEffect is React 18's hook for injecting styles before DOM mutations.
// It runs synchronously before layout effects.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: In TUI context, useInsertionEffect runs similar to useLayoutEffect
// but is prioritized for style injection.

import React, { useInsertionEffect, useState, useRef } from 'react';
import { Box, Text } from 'ink';

// Track when insertion effect runs
let insertionCount = 0;

export default function App() {
  const [ready, setReady] = useState(false);
  const [renderCount, setRenderCount] = useState(0);
  const ref = useRef<number>(0);

  // useInsertionEffect - runs before layout effect
  useInsertionEffect(() => {
    insertionCount++;
    ref.current = insertionCount;
    // In TUI, we simulate style injection
  }, []);

  // Track renders
  useInsertionEffect(() => {
    setRenderCount(prev => prev + 1);
  }, []);

  // Force a re-render after a delay to show effect ordering
  React.useEffect(() => {
    const timeout = setTimeout(() => {
      setReady(true);
    }, 10);
    return () => clearTimeout(timeout);
  }, []);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useInsertionEffect Demo</Text>
      <Text dimColor>React 18 hook for style injection</Text>
      <Text></Text>

      <Text>Insertion effect status:</Text>
      <Text>  last run count: {ref.current}</Text>
      <Text>  current ready: {ready ? 'yes' : 'no'}</Text>

      <Text></Text>
      <Text>Effect ordering (TUI context):</Text>
      <Text>  1. useInsertionEffect (first)</Text>
      <Text>  2. useLayoutEffect (second)</Text>
      <Text>  3. useEffect (last, async)</Text>

      <Text></Text>
      <Text>Note: In TUI, useInsertionEffect behaves</Text>
      <Text>like useLayoutEffect since there's no DOM.</Text>
    </Box>
  );
}
