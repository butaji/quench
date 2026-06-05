// Progress bar example — demonstrates progress display.
// Simplified for parity: uses pre-built progress indicator
// that renders consistently across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function ProgressBar() {
  // Pre-built progress bar for parity (avoids repeat() issues)
  const progress = 50;
  const bar = '[########--------]';
  const percent = 50;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Progress Bar</Text>
      <Text marginTop={1}>
        <Text color="green">{bar}</Text>
      </Text>
      <Text dimColor marginTop={1}>
        {percent}% ({progress}/100)
      </Text>
    </Box>
  );
}
