// ink-array-reduce example — demonstrates Array.prototype.reduce and reduceRight.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function ArrayReduceDemo() {
  const nums = [1, 2, 3, 4, 5];
  const sum = nums.reduce((acc, n) => acc + n, 0);
  const product = nums.reduce((acc, n) => acc * n, 1);
  const max = nums.reduce((acc, n) => (n > acc ? n : acc), nums[0]);
  const reversed = nums.reduceRight((acc, n) => [...acc, n], [] as number[]);

  const entries = [['a', 1], ['b', 2], ['c', 3]] as [string, number][];
  const obj = entries.reduce((acc, [k, v]) => ({ ...acc, [k]: v }), {} as Record<string, number>);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Array Reduce Demo</Text>
      <Text dimColor>reduce and reduceRight</Text>
      <Text></Text>
      <Text>Sum: {sum}</Text>
      <Text>Product: {product}</Text>
      <Text>Max: {max}</Text>
      <Text>Reversed: {reversed.join(', ')}</Text>
      <Text>Keys: {Object.keys(obj).join(', ')}</Text>
    </Box>
  );
}
