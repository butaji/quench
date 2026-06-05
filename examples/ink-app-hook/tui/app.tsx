// App hook example — exercises `useApp` hook
import React from 'react';
// to access the app instance and call `exit()`
// programmatically. Also demonstrates the
// `useStdin`, `useStdout`, `useStderr` hooks
// for direct stdio access.
//
// Note: this example uses hooks which only
// work in the dev path (rquickjs evaluates
// it as JS) and deno. The build path doesn't
// evaluate JS expressions.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function AppHook() {
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">useApp / useStdio Example</Text>
      <Text>This example demonstrates the app-level hooks.</Text>
      <Text dimColor>Press q to exit (handled by useInput)</Text>
    </Box>
  );
}
