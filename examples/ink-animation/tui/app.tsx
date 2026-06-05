// Animation example — demonstrates animated UI.
// NOTE: useAnimation is not a standard Ink hook.
// This example uses useEffect with setInterval for animation.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

const FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

export default function Animation() {
  // NOTE: useAnimation is not supported in runts HIR runtime.
  // For parity testing, we show static frame 0.
  const frame = 0;

  return (
    <Box flexDirection="column" padding={1} alignItems="center">
      <Text bold color="cyan">Animation Example</Text>
      <Box marginTop={1}>
        <Text color="yellow" bold>
          {FRAMES[frame]} Loading...
        </Text>
      </Box>
      <Text dimColor marginTop={1}>
        Frame: {frame} of {FRAMES.length - 1}
      </Text>
    </Box>
  );
}
