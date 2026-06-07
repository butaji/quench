// Top-Level Await Patterns — demonstrating module-level async initialization
//
// NOTE: True top-level await requires native ES module support.
// This example demonstrates the common pattern of module-level
// initialization that would typically use top-level await.

import React from 'react';
import { Box, Text } from 'ink';

// Simulated top-level await patterns:
// 
// Pattern 1: Config loading (would be `const config = await fetch('/config')`)
// Pattern 2: Dynamic import (would be `const { fn } = await import('./utils')`)
// Pattern 3: Conditional loading (would be `const data = await (condition ? a() : b())`)

// Synchronous equivalents of async-loaded data
const config = { theme: 'dark', lang: 'en', version: '2.0' };
const utils = { formatDate: (d: Date) => d.toISOString(), capitalize: (s: string) => s.toUpperCase() };

// Helper using the "imported" utils
function getFormattedDate(): string {
  return utils.formatDate(new Date());
}

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>Module-Level Initialization</Text>
      <Text>Theme: {config.theme}</Text>
      <Text>Lang: {config.lang}</Text>
      <Text>Version: {config.version}</Text>
      <Text>Date: {getFormattedDate()}</Text>
      <Text>Status: {utils.capitalize('ready')}</Text>
      <Text dimColor>(Synchronous init simulating top-level await)</Text>
    </Box>
  );
}
