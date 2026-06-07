import React from "react";
import { Box, Text } from "ink";

// TypeScript type assertions are erased at runtime
// These should all produce identical output

interface User {
  name: string;
  age: number;
}

const data: unknown = { name: "Alice", age: 30 };
const user = data as User;

const value: string | null = "hello";
const nonNull = value!; // Non-null assertion

const obj = { x: 10, y: "text" } as const;
const num = obj.x satisfies number;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Type Assertions Demo</Text>
      <Text>as: {user.name}</Text>
      <Text>!: {nonNull}</Text>
      <Text>satisfies: {num}</Text>
    </Box>
  );
}
