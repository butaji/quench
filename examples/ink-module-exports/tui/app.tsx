// ink-module-exports example — demonstrates module patterns in TS/TSX.
//
// This example uses a single file to demonstrate module-like patterns
// that work in rquickjs eval context. In a full ES module environment,
// multi-file modules with named exports would work differently.
//
// For multi-file module support, see the compile path (runts build).

import React from 'react';
import { Box, Text, Newline } from 'ink';

// Simulated module constants (would be in a separate utils.ts file in ES modules)
const VERSION = '1.0';
const PI = 3.14159;

// Simulated module functions
function format(n: number): string {
  return `#${n}`;
}

function greet(name: string): string {
  return `Hi ${name}`;
}

function uppercase(s: string): string {
  return s.toUpperCase();
}

// Simulated named imports from a module
const utils = {
  VERSION,
  PI,
  format,
  greet,
  uppercase
};

// Simulated re-export
const exportedFormat = format;
const exportedVersion = VERSION;

// Demonstrate the patterns
const directDefault = greet('World');
const namedVersion = VERSION;
const namedFormat = format(42);
const namespaceUpper = utils.uppercase('hello');
const namespaceVersion = utils.VERSION;
const namespaceFormat = utils.format(99);
const namespacePi = utils.PI;

export default function ModuleExportsDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Module Exports Demo</Text>
      <Newline />
      <Text>Direct function call: {directDefault}</Text>
      <Text>Named import VERSION: {namedVersion}</Text>
      <Text>Named import format(42): {namedFormat}</Text>
      <Newline />
      <Text bold>Namespace/object imports:</Text>
      <Text>  utils.uppercase('hello'): {namespaceUpper}</Text>
      <Text>  utils.VERSION: {namespaceVersion}</Text>
      <Text>  utils.format(99): {namespaceFormat}</Text>
      <Text>  utils.PI: {namespacePi.toFixed(2)}</Text>
      <Newline />
      <Text dimColor>Named, default, namespace patterns all simulated.</Text>
    </Box>
  );
}
