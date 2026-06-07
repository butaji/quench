// Animation example — demonstrates useAnimation hook.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (compile path)

import React from 'react';
import { Box, Text, useAnimation } from 'ink';

export default function Animation() {
  const anim = useAnimation({ fps: 10 });
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  const spinner = frames[anim.frame % frames.length];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Animation Demo</Text>
      <Text>Frame: {anim.frame}</Text>
      <Text color="green">{spinner}</Text>
      <Text dimColor>Status: {anim.isPlaying ? 'Playing' : 'Stopped'}</Text>
    </Box>
  );
}

// Import render for testing compatibility
import { render } from 'ink';
render(<Animation />);
