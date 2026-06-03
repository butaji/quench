// Aligned example — pure Ink source that
// exercises the `gap`, `marginTop`, and
// `alignItems="center"` props for box layout.
// Renders a centered title with a wrapped
// description and a right-aligned footer.
//
// All three environments must produce the
// same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Aligned() {
  return (
    <Box flexDirection="column" borderStyle="double" paddingX={2} paddingY={1}>
      <Box flexDirection="row" justifyContent="center">
        <Text bold color="magenta">Centered Title</Text>
      </Box>
      <Box flexDirection="row" justifyContent="flex-end">
        <Text dimColor>right-aligned</Text>
      </Box>
      <Text>Body text spans the full width of the box.</Text>
    </Box>
  );
}
