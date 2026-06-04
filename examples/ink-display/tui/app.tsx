// Display example — exercises `display="none"`
// to hide a Box from the layout (it takes no
// space and is not rendered).
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Display() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text>Visible item 1</Text>
      <Box display="none">
        <Text>Hidden item (display=none)</Text>
      </Box>
      <Text>Visible item 2</Text>
      <Box display="none">
        <Text>Also hidden</Text>
      </Box>
      <Text>Visible item 3</Text>
    </Box>
  );
}
