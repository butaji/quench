// dangerouslySetInnerHTML example — exercises JSX object prop parsing.
// Ink is a terminal renderer and does not support HTML injection,
// but this prop exercises the parser's handling of object-valued JSX attributes.

import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text dangerouslySetInnerHTML={{ __html: 'Raw HTML Content' }}>
        Fallback text
      </Text>
      <Text>Object prop parsed successfully</Text>
    </Box>
  );
}
