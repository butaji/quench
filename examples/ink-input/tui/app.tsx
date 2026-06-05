// Input example — pure Ink source that shows
import React from 'react';
// a prompt with a label and current value.
// Exercises the `bold`, `underline`, and
// `dimColor` props on Text.
//
// Renders a round-bordered box with a
// "User input" title, a value line, and a
// footer hint.
//
// All three environments must produce the
// same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Input() {
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold underline>User input</Text>
      <Text>Type something here</Text>
      <Text dimColor>Press Enter to submit, q to quit.</Text>
    </Box>
  );
}
