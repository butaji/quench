import React from 'react';
import { render, Box, Text } from 'ink';

const values = ['42', '3.14', 'hello', 'Infinity', 'NaN'];

const parsed = values.map(v => ({
  input: v,
  isNum: Number.isFinite(Number(v)),
  isNaN: Number.isNaN(Number(v)),
  parsedInt: Number.parseInt(v, 10),
  parsedFloat: Number.parseFloat(v),
}));

const App = () => (
  <Box flexDirection="column">
    {parsed.map((p, i) => (
      <Text key={i}>{p.input}: finite={p.isNum ? 'yes' : 'no'}, NaN={p.isNaN ? 'yes' : 'no'}, int={p.parsedInt}, float={p.parsedFloat}</Text>
    ))}
    <Text>Epsilon: {Number.EPSILON}</Text>
    <Text>Max Safe: {Number.MAX_SAFE_INTEGER}</Text>
    <Text>Min Safe: {Number.MIN_SAFE_INTEGER}</Text>
  </Box>
);

render(<App />);
