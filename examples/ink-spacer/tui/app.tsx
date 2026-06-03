import { Box, Text, Newline, Spacer } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Text>First line</Text>
      <Newline />
      <Text>Second line after newline</Text>
      <Spacer />
      <Box flexDirection="row" width={50}>
        <Text>Left</Text>
        <Box flexGrow={1}><Text> </Text></Box>
        <Text>Right</Text>
      </Box>
    </Box>
  );
}
