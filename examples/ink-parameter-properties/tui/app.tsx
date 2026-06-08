// Parameter Properties example — constructor(public x: string)
//
// TypeScript parameter properties declare and initialize class
// members directly in the constructor signature.

import React from 'react';
import { Box, Text } from 'ink';

class User {
  constructor(
    public name: string,
    public age: number,
    private readonly id: string,
    protected role: string = 'user'
  ) {}

  describe(): string {
    return `${this.name} (${this.age}) [${this.id}]`;
  }

  getRole(): string {
    return this.role;
  }
}

const user = new User('Alice', 30, 'u-123');

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>Parameter Properties Demo</Text>
      <Text>{user.describe()}</Text>
      <Text>Name: {user.name}</Text>
      <Text>Age: {user.age}</Text>
      <Text>Role: {user.getRole()}</Text>
    </Box>
  );
}
