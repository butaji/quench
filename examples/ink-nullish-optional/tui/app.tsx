// ink-nullish-optional — exercises ?? and ?.
// Both features are fully supported:
// - ?? (nullish coalescing) via LogicalOp::NullishCoalescing in HIR
// - ?. (optional chaining) via optional: true on StaticMember/Member/Call in HIR

import React from 'react';
import { Box, Text } from 'ink';

interface Config {
  theme?: {
    name?: string;
    colors?: string[];
  };
}

const FULL_CONFIG: Config = {
  theme: {
    name: 'dark',
    colors: ['#000000', '#ffffff'],
  },
};

const NULL_CONFIG: Config = {};

const PARTIAL_CONFIG: Config = {
  theme: {
    name: 'light',
  },
};

export default function InkNullishOptional() {
  // Nullish coalescing: ?? returns right side only when left is null/undefined
  const name1 = FULL_CONFIG.theme?.name ?? 'default';
  const name2 = NULL_CONFIG.theme?.name ?? 'default';
  const name3 = PARTIAL_CONFIG.theme?.name ?? 'default';

  // Optional chaining with nullish coalescing
  const color1 = FULL_CONFIG.theme?.colors?.[0] ?? '#ff0000';
  const color2 = NULL_CONFIG.theme?.colors?.[0] ?? '#ff0000';
  const color3 = PARTIAL_CONFIG.theme?.colors?.[0] ?? '#ff0000';

  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Text bold>Nullish Coalescing &amp; Optional Chaining</Text>
      <Box flexDirection="column">
        <Text>Full config name: {name1}</Text>
        <Text>Null config name: {name2}</Text>
        <Text>Partial config name: {name3}</Text>
      </Box>
      <Box flexDirection="column">
        <Text color={color1}>Full color: {color1}</Text>
        <Text color={color2}>Null color: {color2}</Text>
        <Text color={color3}>Partial color: {color3}</Text>
      </Box>
    </Box>
  );
}
