import React from 'react';
import { render, Box, Text } from 'ink';

const id = Symbol('app-id');
const map = new Map<string, number>();
map.set('alpha', 1);
map.set('beta', 2);
map.set('gamma', 3);

const set = new Set<string>(['apple', 'banana', 'cherry']);

const App = () => (
  <Box flexDirection="column">
    <Text>Map size: {map.size}</Text>
    <Text>Map keys: {Array.from(map.keys()).join(', ')}</Text>
    <Text>Set has 'apple': {set.has('apple') ? 'yes' : 'no'}</Text>
    <Text>Set size: {set.size}</Text>
    <Text>Symbol type: {typeof id}</Text>
    <Text>Symbol description: {String(id)}</Text>
  </Box>
);

render(<App />);
