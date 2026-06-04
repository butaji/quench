// Flex reverse example — exercises
// `flexDirection: "row-reverse"` and
// `flexDirection: "column-reverse"` to lay
// out children in reverse order.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function FlexReverse() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Box flexDirection="row" width={30}>
        <Text>A</Text>
        <Text>B</Text>
        <Text>C</Text>
      </Box>
      <Box flexDirection="row-reverse" width={30}>
        <Text>A</Text>
        <Text>B</Text>
        <Text>C</Text>
      </Box>
      <Box flexDirection="row" width={30}>
        <Text>top</Text>
        <Text>bottom</Text>
      </Box>
      <Box flexDirection="row-reverse" width={30}>
        <Text>top</Text>
        <Text>bottom</Text>
      </Box>
    </Box>
  );
}
