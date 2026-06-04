// Dimensions example — exercises `width`,
// `height`, `minWidth`, `aspectRatio` on Box.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Dimensions() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Box width={20} height={3} borderStyle="round" paddingX={1}>
        <Text>20x3 box</Text>
      </Box>
      <Box width={40} borderStyle="round" paddingX={1}>
        <Text>width 40</Text>
      </Box>
      <Box minWidth={30} borderStyle="round" paddingX={1}>
        <Text>minWidth 30</Text>
      </Box>
    </Box>
  );
}
