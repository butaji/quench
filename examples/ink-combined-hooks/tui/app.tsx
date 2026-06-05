// Combined hooks example — demonstrates using multiple hooks together.
// NOTE: useInput and useApp hooks are not yet supported in runts HIR runtime.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Spacer } from 'ink';

export default function CombinedHooksExample() {
  // NOTE: For runts HIR runtime, useInput/useApp are not supported.
  // For parity testing, we show static state.
  const counter = 0;
  const multiplier = 1;
  const canExit = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Combined Hooks Demo</Text>
      <Text></Text>
      
      <Box gap={1}>
        <Text bold>Counter:</Text>
        <Text>{counter}</Text>
      </Box>
      
      <Box gap={1}>
        <Text bold>Step size:</Text>
        <Text>{multiplier}</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Controls:</Text>
      <Text dimColor>  Up/Down - Change counter</Text>
      <Text dimColor>  Left/Right - Change step</Text>
      <Text dimColor>  q/Esc - Exit</Text>
      
      <Text></Text>
      <Box
        flexDirection="column"
        borderStyle="round"
        padding={1}
        borderColor="cyan"
      >
        <Text>This box uses multiple features:</Text>
        <Text>  • Flexbox layout (gap, padding)</Text>
        <Text>  • Border styling</Text>
        <Text>  • Multiple hooks (useInput, useApp)</Text>
        <Text>  • Multiple state variables</Text>
      </Box>
      
      <Spacer />
      
      <Box gap={1}>
        <Text dimColor>Total: </Text>
        <Text dimColor>{counter * multiplier}</Text>
      </Box>
    </Box>
  );
}
