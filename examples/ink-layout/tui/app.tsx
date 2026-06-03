// Layout example — pure Ink source for horizontal
// Box layout. Same .tsx runs in all 3 environments.

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
