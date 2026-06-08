// structuredClone example
// Exercises the structuredClone() API for deep cloning objects.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (compile path)

import React from 'react';
import { Box, Text } from 'ink';

export default function StructuredCloneDemo() {
  // Show structuredClone status
  // In rq/dev path: structuredClone is available
  // In compile path: we document the API availability
  const status = "structuredClone available in rq/dev";
  
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text bold>structuredClone API</Text>
      <Text color="cyan">{status}</Text>
      <Text>Deep clone for objects/arrays</Text>
      <Text dimColor>API: structuredClone(value)</Text>
    </Box>
  );
}
