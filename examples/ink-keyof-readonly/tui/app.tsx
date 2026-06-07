// keyof and readonly example — TypeScript utility operators
//
// keyof extracts keys from a type
// readonly marks arrays/tuples as immutable

import React from 'react';
import { Box, Text } from 'ink';

interface Settings {
  theme: string;
  width: number;
  height: number;
}

// keyof extracts the union of property names
type SettingKey = keyof Settings;

// Readonly arrays - cannot be modified at runtime
const keys: readonly SettingKey[] = ['theme', 'width', 'height'];

// Readonly tuple
const tuple: readonly [string, number, boolean] = ['config', 80, true];

// Readonly array of numbers
const sizes: readonly number[] = [640, 800, 1024, 1280];

const settings: Settings = { theme: 'dark', width: 80, height: 24 };

// Function using keyof
function getSetting(key: SettingKey): string | number {
  return settings[key];
}

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>keyof and readonly Demo</Text>
      <Text>Keys: {keys.join(', ')}</Text>
      <Text>Tuple: {tuple[0]} {tuple[1]} {tuple[2] ? 'enabled' : 'disabled'}</Text>
      <Text>Sizes: {sizes.join('x')}</Text>
      <Text>Theme: {getSetting('theme')}</Text>
      <Text>Size: {getSetting('width')}x{getSetting('height')}</Text>
      <Text dimColor>(keyof/readonly erased at compile time)</Text>
    </Box>
  );
}
