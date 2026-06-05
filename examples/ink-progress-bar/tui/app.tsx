// Progress bar example — demonstrates animated progress display.
// NOTE: useAnimation is not a standard Ink hook.
// This example shows a static progress bar for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function ProgressBar() {
  // NOTE: For runts HIR runtime, animation is not supported.
  // For parity testing, we show static progress.
  const progress = 45;
  const total = 100;
  const barWidth = 30;
  const filledWidth = Math.round((progress / total) * barWidth);
  const emptyWidth = barWidth - filledWidth;

  const filled = '█'.repeat(filledWidth);
  const empty = '░'.repeat(emptyWidth);
  const bar = filled + empty;
  const percent = Math.round((progress / total) * 100);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Progress Bar</Text>
      <Box marginTop={1}>
        <Text>
          <Text color="green">{bar}</Text> {percent}%
        </Text>
      </Box>
      <Text dimColor marginTop={1}>
        Progress: {progress} / {total}
      </Text>
    </Box>
  );
}
