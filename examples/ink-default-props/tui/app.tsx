// Default props example — exercises default parameter values.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

interface GreetingProps {
  name: string;
  greeting?: string;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Greeting name="Alice" />
      <Greeting name="Bob" greeting="Hi" />
    </Box>
  );
}

function Greeting({ name, greeting = 'Hello' }: GreetingProps) {
  return <Text>{greeting}, {name}!</Text>;
}
