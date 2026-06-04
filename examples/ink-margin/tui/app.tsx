// Margin example — exercises `marginTop`,
// `marginBottom`, `marginLeft`, `marginRight`,
// `marginX`, `marginY` on Box.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Margin() {
  return (
    <Box flexDirection="column" borderStyle="single" paddingX={2} paddingY={1}>
      <Text>Header</Text>
      <Box marginTop={1} marginBottom={1}>
        <Text>Spaced section</Text>
      </Box>
      <Box marginX={4}>
        <Text>Indented</Text>
      </Box>
      <Box marginY={1}>
        <Text>Vertically spaced</Text>
      </Box>
    </Box>
  );
}
