import { Box, Text } from 'ink';

export default function App() {
  const isActive = true;
  const count = 3;
  const items = ['first', 'second', 'third'];
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="round">
      <Text color={isActive ? 'green' : 'red'}>
        Status: {isActive ? 'ACTIVE' : 'INACTIVE'}
      </Text>
      <Text>Count: {count}</Text>
      <Box flexDirection="column" marginTop={1}>
        {items.map((item, i) => (
          <Text key={i}>Item {i + 1}: {item}</Text>
        ))}
      </Box>
    </Box>
  );
}
