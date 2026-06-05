// Hooks overview example — demonstrates all Ink hooks available.
// NOTE: Most hooks are not supported in runts HIR runtime.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function HooksOverview() {
  // NOTE: Hooks are not supported in runts HIR runtime.
  // Static values shown for parity testing.
  const stdinAvailable = true;
  const stdoutAvailable = true;
  const stderrAvailable = true;
  const rawModeSupported = false;
  const columns = 80;
  const rows = 24;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Hooks Overview</Text>
      <Text></Text>
      
      <Box flexDirection="column" gap={1}>
        <Text bold>useStdin:</Text>
        <Text dimColor>  stdin available: {stdinAvailable ? 'Yes' : 'No'}</Text>
        <Text dimColor>  raw mode supported: {rawModeSupported ? 'Yes' : 'No'}</Text>
        
        <Text bold>useStdout:</Text>
        <Text dimColor>  stdout available: {stdoutAvailable ? 'Yes' : 'No'}</Text>
        
        <Text bold>useStderr:</Text>
        <Text dimColor>  stderr available: {stderrAvailable ? 'Yes' : 'No'}</Text>
        
        <Text bold>useWindowSize:</Text>
        <Text dimColor>  columns: {columns}</Text>
        <Text dimColor>  rows: {rows}</Text>
      </Box>
      
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text dimColor italic>
          Hooks not yet supported in runts dev mode.
        </Text>
      </Box>
    </Box>
  );
}
