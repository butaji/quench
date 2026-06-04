// Border color example — exercises
// `borderColor`, `borderTopColor`,
// `borderBottomColor`, `borderLeftColor`,
// `borderRightColor`, `borderDimColor` on Box.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function BorderColor() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Box borderStyle="round" borderColor="green" paddingX={1}>
        <Text>green border</Text>
      </Box>
      <Box borderStyle="round" borderTopColor="red" borderBottomColor="blue" paddingX={1}>
        <Text>red top, blue bottom</Text>
      </Box>
      <Box borderStyle="round" borderDimColor paddingX={1}>
        <Text>dim border</Text>
      </Box>
    </Box>
  );
}
