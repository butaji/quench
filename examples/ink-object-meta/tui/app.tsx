import React from 'react';
import { Box, Text } from 'ink';

// Object meta methods demo
const proto = { type: 'widget' };
const obj = Object.create(proto);
obj.name = 'Button';

const frozen = Object.freeze({ x: 1 });
const sealed = Object.seal({ y: 2 });
const merged = Object.assign({}, { a: 1 }, { b: 2 });

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Object.create, freeze, seal, assign Demo</Text>
      <Text>Proto type: {obj.type}</Text>
      <Text>Name: {obj.name}</Text>
      <Text>Frozen x: {frozen.x}</Text>
      <Text>Sealed y: {sealed.y}</Text>
      <Text>Merged keys: {Object.keys(merged).join(', ')}</Text>
      <Text>Merged a: {merged.a}</Text>
      <Text>Merged b: {merged.b}</Text>
    </Box>
  );
}
