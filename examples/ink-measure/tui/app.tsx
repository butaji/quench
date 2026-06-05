// Measure example — exercises the useBoxMetrics hook.
// Demonstrates measuring element dimensions at runtime.
//
// This example shows how to measure the rendered size
// of elements using useBoxMetrics.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React, { useRef } from 'react';
import { Box, Text, useBoxMetrics } from 'ink';

export default function Measure() {
  const ref = useRef<HTML.BoxElement>(null);
  const metrics = useBoxMetrics(ref);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Box Metrics</Text>
      <Box ref={ref} marginTop={1} padding={1} borderStyle="round">
        <Text>Measured box</Text>
      </Box>
      <Box marginTop={1}>
        <Text>
          <Text bold>Width:</Text> {metrics?.width ?? 'N/A'}
        </Text>
      </Box>
      <Box>
        <Text>
          <Text bold>Height:</Text> {metrics?.height ?? 'N/A'}
        </Text>
      </Box>
    </Box>
  );
}
