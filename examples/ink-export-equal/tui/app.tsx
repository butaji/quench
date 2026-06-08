import React from 'react';
import { Box, Text } from 'ink';

// Inline module exports (equivalent to TypeScript's export = and import = require())
function createGreeting(name: string): string {
  return "Hello, " + name + "!";
}

function createFarewell(name: string): string {
  return "Goodbye, " + name + "!";
}

var utils = {
  formatName: function(name: string): string { return name.toUpperCase(); },
  getYear: function(): number { return 2026; }
};

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text>Greeting: {createGreeting("World")}</Text>
      <Text>Farewell: {createFarewell("World")}</Text>
      <Text>Uppercase: {utils.formatName("test")}</Text>
      <Text>Year: {utils.getYear()}</Text>
    </Box>
  );
}
