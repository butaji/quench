// Standard ES module imports
import { createGreeting, createFarewell, utils } from '../lib/lib.ts';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text>Using ES module imports:</Text>
      <Text>{createGreeting('World')}</Text>
      <Text>{createFarewell('World')}</Text>
      <Text>---</Text>
      <Text>Using imported utils:</Text>
      <Text>Uppercase name: {utils.formatName('test')}</Text>
    </Box>
  );
}
