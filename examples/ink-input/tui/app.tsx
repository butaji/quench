// Input example — pure Ink source that shows
// a prompt with a label and current value.
// Exercises the `<Text>` with multiple lines
// and the `wrap` prop on Box.
//
// Renders a labelled, wrapped text block with
// a "User input: <text>" prompt and a helper
// line.
//
// All three environments must produce the
// same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Input({ value = 'Type something here' }) {
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold underline>User input</Text>
      <Text>{value}</Text>
      <Text dimColor>Press Enter to submit, q to quit.</Text>
    </Box>
  );
}
