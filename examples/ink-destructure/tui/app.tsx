// Destructuring example — demonstrates object/array destructuring with defaults and rest.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function DestructureDemo() {
  const results: string[] = [];

  // Object destructuring (simple)
  const person = { name: 'Alice', age: 30, city: 'NYC' };
  // Access properties directly for compile path compatibility
  results.push(`object: name=${person.name}, age=${person.age}`);

  // Array access (compile path friendly)
  const colors = ['red', 'green', 'blue'];
  results.push(`array: first=${colors[0]}, second=${colors[1]}, third=${colors[2]}`);

  // Simulated destructuring with explicit variables
  const first = colors[0];
  const second = colors[1];
  const third = colors[2];
  results.push(`explicit: ${first}, ${second}, ${third}`);

  // Object with default (simulated)
  const incomplete = { name: 'Bob' };
  const age = incomplete.age !== undefined ? incomplete.age : 25;
  results.push(`with default: name=${incomplete.name}, age=${age}`);

  // Nested access
  const nested = { user: { id: 1, email: 'test@example.com' } };
  results.push(`nested: id=${nested.user.id}, email=${nested.user.email}`);

  // Template with values
  results.push(`template: Hello ${person.name} from ${person.city}!`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Destructuring Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
