// Animation example — demonstrates useAnimation hook.
// Simplified for parity: static frame output

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
