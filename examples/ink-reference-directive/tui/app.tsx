/// <reference types="./types/my-types.d.ts" />
/// <reference path="./types/my-types.d.ts" />

// Type declarations are erased at compile time.
// The reference directive is stripped during transpilation.
// This example verifies that reference directives are handled
// without causing errors in any environment.

import React from 'react';
import { Box, Text } from 'ink';

// These type annotations are stripped during transpilation
type MyConfig = {
  version: string;
  count: number;
};

interface Options {
  name: string;
  value: number;
}

export default function ReferenceDirective() {
  // Inline the logic that would be in a helper function.
  // Type information erased, runtime code preserved.
  const opts: Options = { name: "config", value: 42 };
  const result = opts.name + ": " + opts.value;

  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text bold>TypeScript Reference Directive</Text>
      <Text>Reference types stripped during compile</Text>
      <Text color="green">Options: {result}</Text>
      <Text dimColor>No runtime type checking needed</Text>
    </Box>
  );
}
