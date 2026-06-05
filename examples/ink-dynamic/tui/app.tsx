// Dynamic example — pure Ink source. Shows
import React from 'react';
// different status states in a bordered box.
// Exercises `color`, `bold`, `dimColor`
// on Text.
//
// Renders a bordered box with three
// Text rows: title, status (with
// state-derived color), and a footer hint.
//
// All three environments must produce the
// same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Dynamic() {
  return (
    <Box flexDirection="column" borderStyle="single" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Live Status</Text>
      <Text color="green">OK</Text>
      <Text color="yellow">WARN</Text>
      <Text color="red">FAIL</Text>
      <Text dimColor>Press r to refresh, q to quit.</Text>
    </Box>
  );
}
