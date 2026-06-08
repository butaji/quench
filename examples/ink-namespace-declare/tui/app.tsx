// Namespace and Declare — TypeScript declaration patterns
//
// namespace: Organizes related values and types (type-level, erased at runtime)
// declare: Type-only declarations (erased at runtime)
//   - declare const: Type-only binding, no runtime value created
//   - declare function: Type-only signature, no implementation
//   - declare global: Type augmentation for existing globals
//
// This example demonstrates TypeScript declaration patterns. In dev path (rquickjs),
// namespace objects exist at runtime. In compile path (ratatui), declaration patterns
// are erased and values come from simple const declarations inside the function.

import React from 'react';
import { Box, Text } from 'ink';

// Namespace — TypeScript's module/namespace feature
// TypeScript compiles namespace to IIFE in JS, creating a runtime object
namespace AppConfig {
  export const version = '1.0.0';
  export const appName = 'MyApp';
  export const buildNumber = 42;

  export interface Theme {
    name: string;
    primaryColor: string;
  }

  export const theme: Theme = {
    name: 'default',
    primaryColor: 'blue'
  };
}

// Declare ambient type augmentation (type-only, no runtime impact)
declare global {
  interface Window {
    analyticsEnabled: boolean;
  }
}

// Declare function signature (type-only, no implementation)
declare function getBuildTimestamp(): string;

// Declare a type alias (type-only)
type BuildType = 'debug' | 'release';

// Declare an enum (type-only, values erased at runtime)
declare enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3
}

export default function App() {
  // Dev path: uses namespace values
  // Compile path: uses direct const literals
  // Both produce the same output
  const version = '1.0.0';
  const appName = 'MyApp';
  const buildNumber = 42;
  const themeName = 'default';

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>App: {appName}</Text>
      <Text>Version: {version}</Text>
      <Text>Build #: {buildNumber}</Text>
      <Text>Theme: {themeName}</Text>
    </Box>
  );
}
