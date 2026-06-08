// ink-promise-advanced example — allSettled, any, race, withResolvers
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Demonstrate Promise static methods exist
const hasAllSettled = 'allSettled' in Promise;
const hasAny = 'any' in Promise;
const hasRace = 'race' in Promise;
const hasWithResolvers = 'withResolvers' in Promise;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>allSettled: {hasAllSettled ? 'yes' : 'no'}</Text>
      <Text>any: {hasAny ? 'yes' : 'no'}</Text>
      <Text>race: {hasRace ? 'yes' : 'no'}</Text>
      <Text>withResolvers: {hasWithResolvers ? 'yes' : 'no'}</Text>
    </Box>
  );
}
