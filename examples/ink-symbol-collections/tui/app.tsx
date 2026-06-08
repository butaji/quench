// ink-symbol-collections example — Symbol, Map, Set, WeakMap
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

const id = Symbol('app-id');
const map = new Map<string, number>();
map.set('alpha', 1);
map.set('beta', 2);
map.set('gamma', 3);

const set = new Set<string>(['apple', 'banana', 'cherry']);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Map size: {map.size}</Text>
      <Text>Map keys: {Array.from(map.keys()).join(', ')}</Text>
      <Text>Set has 'apple': {set.has('apple') ? 'yes' : 'no'}</Text>
      <Text>Set size: {set.size}</Text>
      <Text>Symbol type: {typeof id}</Text>
      <Text>Symbol description: {String(id)}</Text>
    </Box>
  );
}
