// Layout example — pure Ink source for
// horizontal Box layout. Exercises the
// `flexDirection="row"` path of the
// runts-ink renderer.
//
// Renders three Text columns side-by-side
// inside a bordered Box with single-line
// borders and 1-char horizontal padding.
//
// All three environments must produce
// identical output:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Layout() {
  return (
    <Box flexDirection="row" borderStyle="single" paddingX={1}>
      <Text>Left</Text>
      <Text>Center</Text>
      <Text>Right</Text>
    </Box>
  );
}
