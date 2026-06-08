// ink-string-modern example — padStart, padEnd, replaceAll, trimStart, trimEnd, at
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

const id = '42'.padStart(6, '0');
const label = 'App'.padEnd(10, '.');
const text = 'hello world hello'.replaceAll('hello', 'hi');
const spaced = '  trim  '.trimStart().trimEnd();
const word = 'hello';
const first = word.at(0);
const last = word.at(-1);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>ID: {id}</Text>
      <Text>Label: {label}</Text>
      <Text>Replaced: {text}</Text>
      <Text>Trimmed: {spaced}</Text>
      <Text>First: {first}, Last: {last}</Text>
    </Box>
  );
}
