// Global and module augmentation example.
// Demonstrates TypeScript's declare global and declare module patterns.
//
// These type declarations are erased at compile time, so the runtime
// behavior is the same across all environments.

import React from 'react';
import { Box, Text } from 'ink';

// Check for __BUILD_TIME__ which would be declared via global augmentation
// In TypeScript: declare var __BUILD_TIME__: string;
// At runtime, this would be undefined unless explicitly set
const buildTime = typeof __BUILD_TIME__ !== 'undefined' 
  ? (__BUILD_TIME__ as string)
  : 'development';

// Custom data-testid prop on Box (via module augmentation of 'ink')
// In TypeScript: declare module 'ink' { interface BoxProps { 'data-testid'?: string; } }

export default function GlobalAugmentation() {
  return (
    <Box 
      flexDirection="column" 
      paddingX={2} 
      paddingY={1} 
      borderStyle="single"
      // This prop is declared via module augmentation
      data-testid="root"
    >
      <Text bold>TypeScript Augmentation</Text>
      <Text dimColor>Build: {buildTime}</Text>
      <Text>Global/Module declarations erased</Text>
    </Box>
  );
}
