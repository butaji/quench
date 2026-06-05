// Animation example — exercises the useAnimation hook.
// Displays a spinning animation that updates every 100ms.
//
// The animation cycles through a sequence of frames,
// demonstrating time-based updates without user input.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, useAnimation } from 'ink';

const FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

export default function Animation() {
  const [frame, setFrame] = useState(0);

  useAnimation((frameCount: number, time: number, delta: number) => {
    setFrame(frameCount % FRAMES.length);
  }, { interval: 100 });

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
