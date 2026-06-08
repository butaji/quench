// ink-proxy example — Proxy handler
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Proxy with get and set traps
const createCounter = (initial: number): { value: number } => {
  return new Proxy({ value: initial }, {
    get(target, prop) {
      return Reflect.get(target, prop);
    },
    set(target, prop, newValue) {
      return Reflect.set(target, prop, newValue);
    },
  });
};

const counter = createCounter(10);
counter.value = 42;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {counter.value}</Text>
    </Box>
  );
}
