// Type Aliases and Interfaces example — TypeScript type declarations
//
// TypeScript types are completely erased at compile time.
// This example demonstrates that type aliases, interfaces,
// and interface extension have no runtime impact.

import React from 'react';
import { Box, Text } from 'ink';

// Type alias for primitive union
type Color = 'red' | 'green' | 'blue';

// Interface declaration
interface User {
  name: string;
  age: number;
  favoriteColor: Color;
}

// Interface extension
interface Admin extends User {
  role: 'admin' | 'super';
}

// Type alias for object shape
type Theme = {
  primary: Color;
  secondary: Color;
};

// Using the types at runtime (types are erased)
const admin: Admin = {
  name: 'Alice',
  age: 30,
  favoriteColor: 'blue',
  role: 'admin'
};

const theme: Theme = {
  primary: 'blue',
  secondary: 'green'
};

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>TypeScript Types Demo</Text>
      <Text>User: {admin.name} ({admin.age})</Text>
      <Text>Role: {admin.role}</Text>
      <Text>Color: {admin.favoriteColor}</Text>
      <Text>Theme: {theme.primary} / {theme.secondary}</Text>
      <Text dimColor>(All TS types erased at compile time)</Text>
    </Box>
  );
}
