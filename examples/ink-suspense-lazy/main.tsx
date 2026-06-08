import React from 'react';
import { render, Box, Text, Newline } from 'ink';

const App = () => (
  <Box flexDirection="column">
    <Text bold>React Suspense + Lazy Example</Text>
    <Newline />
    <Box borderStyle="round" borderColor="green">
      <Text>This is loaded lazily!</Text>
    </Box>
    <Newline />
    <Text dimColor>This text appears immediately.</Text>
  </Box>
);

render(<App />);
