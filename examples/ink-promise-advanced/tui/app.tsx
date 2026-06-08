import React from 'react';
import { Box, Text } from 'ink';

// Demonstrate Promise static methods exist
const hasAllSettled = 'allSettled' in Promise;
const hasAny = 'any' in Promise;
const hasRace = 'race' in Promise;
const hasWithResolvers = 'withResolvers' in Promise;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>allSettled: {hasAllSettled ? 'yes' : 'no'}</Text>
      <Text>any: {hasAny ? 'yes' : 'no'}</Text>
      <Text>race: {hasRace ? 'yes' : 'no'}</Text>
      <Text>withResolvers: {hasWithResolvers ? 'yes' : 'no'}</Text>
    </Box>
  );
}
