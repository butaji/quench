import React from 'react';
import { Box, Text } from 'ink';

// JSON.stringify & JSON.parse Demo
const obj = { name: 'MyApp', version: 1 };
const json = JSON.stringify(obj, null, 2);
const reparsed = JSON.parse(json);

// Array parsing
const parsedArray = JSON.parse('[1, 2, 3, 4, 5]');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>JSON.stringify &amp; JSON.parse Demo</Text>
      <Text>Original: app={obj.name}</Text>
      <Text>Reparsed: {reparsed.name === obj.name ? 'true' : 'false'}</Text>
      <Text>Selective: {JSON.stringify(obj, ['name'], 2)}</Text>
      <Text>Parsed array: {JSON.stringify(parsedArray)}</Text>
    </Box>
  );
}
