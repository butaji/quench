// Hooks overview example — demonstrates all Ink hooks available.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { Box, Text, useApp, useStdin, useStdout, useStderr, useWindowSize } from 'ink';

export default function HooksOverview() {
  const [tick, setTick] = useState(0);
  const app = useApp();
  const stdin = useStdin();
  const stdout = useStdout();
  const stderr = useStderr();
  const { width, height } = useWindowSize();

  useEffect(() => {
    setTick(1);
  }, []);

  const info = useMemo(() => ({
    stdinAvailable: !!stdin,
    stdoutAvailable: !!stdout,
    stderrAvailable: !!stderr,
    rawModeSupported: stdin.isRawModeSupported,
    columns: width,
    rows: height,
  }), [stdin, stdout, stderr, width, height]);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Hooks Overview</Text>
      <Text></Text>
      <Box flexDirection="column" gap={1}>
        <Text bold>useStdin:</Text>
        <Text dimColor>  stdin available: {info.stdinAvailable ? 'Yes' : 'No'}</Text>
        <Text dimColor>  raw mode supported: {info.rawModeSupported ? 'Yes' : 'No'}</Text>
        <Text bold>useStdout:</Text>
        <Text dimColor>  stdout available: {info.stdoutAvailable ? 'Yes' : 'No'}</Text>
        <Text bold>useStderr:</Text>
        <Text dimColor>  stderr available: {info.stderrAvailable ? 'Yes' : 'No'}</Text>
        <Text bold>useWindowSize:</Text>
        <Text dimColor>  columns: {info.columns}</Text>
        <Text dimColor>  rows: {info.rows}</Text>
        <Text bold>useApp:</Text>
        <Text dimColor>  app available: {app ? 'Yes' : 'No'}</Text>
      </Box>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text dimColor italic>
          All hooks work in deno and runts build. HIR runtime provides static stubs.
        </Text>
      </Box>
    </Box>
  );
}
