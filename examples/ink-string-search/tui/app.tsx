// String search example — exercises ES2015+ string search methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen)

import React from 'react';
import { Box, Text } from 'ink';

const text = 'Hello, TypeScript World!';
const starts = text.startsWith('Hello');
const ends = text.endsWith('World!');
const has = text.includes('TypeScript');
const missing = text.includes('Rust');
const repeated = '=-='.repeat(5);
const padded = 'hi'.padStart(6, ' ');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Starts with Hello: {starts ? 'yes' : 'no'}</Text>
      <Text>Ends with World!: {ends ? 'yes' : 'no'}</Text>
      <Text>Includes TypeScript: {has ? 'yes' : 'no'}</Text>
      <Text>Includes Rust: {missing ? 'yes' : 'no'}</Text>
      <Text>Repeated: {repeated}</Text>
      <Text>Padded: [{padded}]</Text>
    </Box>
  );
}
