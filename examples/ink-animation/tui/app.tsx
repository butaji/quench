// Animation example — demonstrates animated UI with useState/useEffect.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function Animation() {
  const [frame, setFrame] = useState(0);
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

  useEffect(() => {
    // In real ink this would use an interval; HIR renders initial frame
    setFrame(1);
  }, []);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Animation Demo</Text>
      <Text>Frame: {frame} of {frames.length}</Text>
      <Text color="green">{frames[frame % frames.length]}</Text>
    </Box>
  );
}
