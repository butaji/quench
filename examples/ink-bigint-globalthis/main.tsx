import React from 'react';
import { render, Box, Text } from 'ink';

const big = 9007199254740993n;
const formatted = 1_000_000_000;
const isNode = typeof globalThis.process !== 'undefined';
const isDeno = typeof Deno !== 'undefined';
const platform = isNode ? 'Node.js' : (isDeno ? 'Deno' : 'Browser');

const App = () => (
  <Box flexDirection="column">
    <Text>BigInt: {String(big)}</Text>
    <Text>Numeric separator: {formatted}</Text>
    <Text>Platform: {platform}</Text>
    <Text>Has globalThis: {typeof globalThis === 'object' ? 'yes' : 'no'}</Text>
  </Box>
);

render(<App />);
