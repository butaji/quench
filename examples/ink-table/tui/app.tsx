// Table layout example — demonstrates table-style layouts.
// Uses flexbox to create columns and rows.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function TableLayout() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Scoreboard</Text>
      <Text></Text>
      <Box>
        <Box width={15}>
          <Text bold underline>Name</Text>
        </Box>
        <Box width={12}>
          <Text bold underline>Status</Text>
        </Box>
        <Text bold underline>Score</Text>
      </Box>
      <Text></Text>
      <Box>
        <Box width={15}>
          <Text>Alice</Text>
        </Box>
        <Box width={12}>
          <Text color="cyan">Active</Text>
        </Box>
        <Text>95</Text>
      </Box>
      <Box>
        <Box width={15}>
          <Text>Bob</Text>
        </Box>
        <Box width={12}>
          <Text color="yellow">Away</Text>
        </Box>
        <Text>87</Text>
      </Box>
      <Box>
        <Box width={15}>
          <Text>Charlie</Text>
        </Box>
        <Box width={12}>
          <Text color="cyan">Active</Text>
        </Box>
        <Text>92</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
