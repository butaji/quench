// import type / export type example — Type-only imports and exports
//
// import type and export type are TypeScript patterns for
// type-only imports that are completely erased at compile time.

import React from 'react';
import { Box, Text } from 'ink';

// Inline type definitions (in real code these would be in separate files)
// Type alias declarations
type Status = 'idle' | 'loading' | 'success' | 'error';
type Theme = 'light' | 'dark';

interface Config {
  status: Status;
  theme: Theme;
  title: string;
}

// Type-only re-export (used by other modules)
// export type { Status, Theme };

const cfg: Config = { status: 'success', theme: 'dark', title: 'My App' };

function getStatusLabel(status: Status): string {
  return status.toUpperCase();
}

function getThemeLabel(theme: Theme): string {
  return theme === 'dark' ? 'Dark Mode' : 'Light Mode';
}

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>import/export type Demo</Text>
      <Text>Status: {getStatusLabel(cfg.status)}</Text>
      <Text>Theme: {getThemeLabel(cfg.theme)}</Text>
      <Text>Title: {cfg.title}</Text>
      <Text dimColor>(import type / export type erased)</Text>
    </Box>
  );
}
