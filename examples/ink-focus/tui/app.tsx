// Focus example — demonstrates keyboard navigation between focusable elements.
// NOTE: useFocus and useFocusManager hooks are not yet supported in runts HIR runtime.
// Shows static focus state for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Focus() {
  // NOTE: For runts HIR runtime, useFocus/useFocusManager are not supported.
  // For parity testing, we show static focus state (first button focused).
  const active = 0;

  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Tab to cycle focus:</Text>
      <Box flexDirection="row" marginTop={1}>
        <Text color={active === 0 ? 'green' : undefined}>{'[1] Button A'}</Text>
        <Text color={active === 1 ? 'green' : undefined}>{'  [2] Button B'}</Text>
        <Text color={active === 2 ? 'green' : undefined}>{'  [3] Button C'}</Text>
      </Box>
    </Box>
  );
}
