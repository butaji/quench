// App hook example — demonstrates app-level hooks.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useApp } from 'ink';

export default function AppHook() {
  const app = useApp();
  const canExit = !!app?.exit;

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
