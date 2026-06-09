// Iterator Helpers Demo — demonstrates TC39 Stage 3 iterator helpers
// (Iterator.from, map, filter, take, drop, reduce, toArray)
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

function* range(start: number, end: number) {
  for (let i = start; i <= end; i++) yield i;
}

export default function IteratorHelpersDemo() {
  // Use Iterator.from with map, filter, take
  const iter = Iterator.from(range(1, 10));
  const mapped = iter.map(n => n * 2);
  const filtered = mapped.filter(n => n > 10);
  const taken = Array.from(filtered.take(3));

  // Use drop + reduce
  const dropped = Iterator.from(range(1, 5)).drop(2);
  const sum = dropped.reduce((a, b) => a + b, 0);

  // Use toArray via Array.from on chained helpers
  const evens = Array.from(
    Iterator.from(range(1, 20))
      .filter(n => n % 2 === 0)
      .take(5)
  );

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Iterator Helpers Demo</Text>
      <Text></Text>
      <Text>map*2 filter&gt;10 take(3): {taken.join(', ')}</Text>
      <Text>drop(2) reduce sum(3..5): {sum}</Text>
      <Text>filter even take(5): {evens.join(', ')}</Text>
    </Box>
  );
}
