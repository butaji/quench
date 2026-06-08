// Inline type import example — import { type X }, import type * as ns
//
// TypeScript 4.5+ inline type imports and namespace type imports
// are completely erased at compile time.

import React from 'react';
import { Box, Text } from 'ink';
import { type User, type Status } from '../types.ts';
import type * as Types from '../types.ts';

export default function App() {
  const user: User = { name: 'Alice', age: 30 };
  const status: Status = 'active';
  const altUser: Types.User = { name: 'Bob', age: 25 };

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>Inline Type Import Demo</Text>
      <Text>User: {user.name} ({user.age})</Text>
      <Text>Status: {status}</Text>
      <Text>Alt: {altUser.name} ({altUser.age})</Text>
      <Text dimColor>(type imports erased)</Text>
    </Box>
  );
}
