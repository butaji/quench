import React, { Children } from 'react';
import { Box, Text } from 'ink';

function ItemList({ children }: { children: React.ReactNode }) {
  const count = Children.count(children);
  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Box flexDirection="column">{children}</Box>
    </Box>
  );
}

export default function App() {
  return (
    <ItemList>
      <Text>Apple</Text>
      <Text>Banana</Text>
      <Text>Cherry</Text>
    </ItemList>
  );
}
