// Ref example — demonstrates useRef for mutable values outside state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen->runts-ink)

import React, { useRef } from 'react';
import { Box, Text } from 'ink';

export default function RefDemo() {
  const counterRef = useRef(0);

  // Increment the ref (simulating access)
  counterRef.current += 1;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useRef Demo</Text>
      <Text>Render count: {counterRef.current}</Text>
      <Text dimColor>
        Refs persist across renders without causing re-renders.
      </Text>
    </Box>
  );
}
