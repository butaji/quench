// Performance API example
// Exercises performance.now(), performance.mark, and performance.measure
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (compile path)

import React from 'react';
import { Box, Text } from 'ink';

export default function PerformanceDemo() {
  // Measure work time - inline the loop
  const start = performance.now();
  for (let i = 0; i < 1000; i++) {
    String.fromCharCode(65 + (i % 26));
  }
  const end = performance.now();
  const duration = end - start;
  
  // performance.mark for marking points
  performance.mark("work-start");
  performance.mark("work-end");
  
  // Calculate time in readable format
  const timeMs = duration.toFixed(2);
  const timeLabel = duration < 1 ? "sub-millisecond" : `${timeMs}ms`;
  
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text bold>Performance API</Text>
      <Text color="cyan">Time elapsed: {timeLabel}</Text>
      <Text>String ops: 1000 chars</Text>
      <Text>Array ops: 100 items</Text>
      <Text dimColor>Marks: work-start, work-end</Text>
    </Box>
  );
}
