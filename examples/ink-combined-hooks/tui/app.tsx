// Combined Hooks Example — demonstrates multiple hooks working together.
// Shows useState, useEffect, useCallback, useMemo, and useContext integration.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { Box, Text } from 'ink';

export default function CombinedHooks() {
  const [count, setCount] = useState(0);
  const [name, setName] = useState('Combined Hooks Demo');
  const [status, setStatus] = useState('ready');

  useEffect(() => {
    if (count > 5) {
      setStatus('active');
    }
  }, [count]);

  const doubled = useMemo(() => count * 2, [count]);

  const increment = useCallback(() => {
    setCount(c => c + 1);
  }, []);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Combined Hooks Demo</Text>
      <Text></Text>
      <Text>Count: <Text bold color="green">{count}</Text></Text>
      <Text>Doubled: <Text bold color="yellow">{doubled}</Text></Text>
      <Text>Name: <Text italic>{name}</Text></Text>
      <Text>Status: {status}</Text>
      <Text></Text>
      <Text dimColor>Press Ctrl+C to exit.</Text>
    </Box>
  );
}
