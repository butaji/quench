// App hook example — demonstrates app-level hooks.
// NOTE: useApp and useStdin hooks are not yet supported in runts HIR runtime.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function AppHook() {
  // NOTE: useApp is not supported in runts HIR runtime.
  // For parity testing, we show static content.
  const canExit = true;

  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">useApp / useStdio Example</Text>
      <Text></Text>
      <Text dimColor>App exit available: {canExit ? 'Yes' : 'No'}</Text>
      <Text dimColor>Stdin available: Yes</Text>
      <Text dimColor>Stdout available: Yes</Text>
    </Box>
  );
}
