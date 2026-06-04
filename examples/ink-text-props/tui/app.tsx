// Text props example — exercises `strikethrough`,
// `inverse`, and `wrap` on Text.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function TextProps() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text strikethrough>Deprecated feature</Text>
      <Text inverse>HIGHLIGHTED</Text>
      <Text>
        This is a long line that should wrap to the next line when the box width is constrained to a smaller value than the text length.
      </Text>
    </Box>
  );
}
