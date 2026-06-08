// ink-in-operator example — demonstrates the `in` operator.
//
// The `in` operator returns true if a property exists in an object
// or if an index exists in an array.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Object with various properties
const user = {
  name: 'Alice',
  age: 30,
  email: 'alice@example.com',
  address: {
    city: 'NYC',
    zip: '10001'
  }
};

// Sparse array
const sparse: (string | undefined)[] = [];
sparse[0] = 'first';
sparse[5] = 'sixth';
sparse[10] = 'eleventh';

interface Config {
  debug?: boolean;
  maxItems?: number;
  timeout?: number;
}

// Function checking optional properties
function hasOptionalProp<T extends object, K extends keyof T>(
  obj: T, key: K
): boolean {
  return key in obj;
}

export default function App() {
  // Basic object property checks
  const hasName = 'name' in user;
  const hasAge = 'age' in user;
  const hasMissing = 'missing' in user;
  const hasToString = 'toString' in user; // inherited property

  // Nested property check (manual)
  const hasAddress = 'address' in user;
  const hasCity = hasAddress && 'city' in user.address;

  // Array index checks
  const arr = ['a', 'b', 'c'];
  const hasIndex0 = 0 in arr;
  const hasIndex1 = 1 in arr;
  const hasIndex5 = 5 in arr;
  const hasLength = 'length' in arr;

  // Sparse array checks
  const sparseHas0 = 0 in sparse;
  const sparseHas5 = 5 in sparse;
  const sparseHas3 = 3 in sparse; // undefined but exists
  const sparseHas7 = 7 in sparse; // doesn't exist

  // Optional property checks
  const config: Config = { debug: true };
  const hasDebug = 'debug' in config;
  const hasMaxItems = 'maxItems' in config;
  const hasTimeout = 'timeout' in config;

  // Using hasOptionalProp function
  const userHasName = hasOptionalProp(user, 'name');
  const userHasEmail = hasOptionalProp(user, 'email');
  const configHasDebug = hasOptionalProp(config, 'debug');

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">in Operator Demo</Text>
      <Text dimColor>Checking property existence</Text>
      <Text></Text>

      <Text>Object property checks:</Text>
      <Text>  'name' in user: {String(hasName)}</Text>
      <Text>  'age' in user: {String(hasAge)}</Text>
      <Text>  'missing' in user: {String(hasMissing)}</Text>
      <Text>  'toString' in user: {String(hasToString)} (inherited)</Text>

      <Text></Text>
      <Text>Nested object checks:</Text>
      <Text>  'address' in user: {String(hasAddress)}</Text>
      <Text>  'city' in user.address: {String(hasCity)}</Text>

      <Text></Text>
      <Text>Array index checks:</Text>
      <Text>  0 in ['a','b','c']: {String(hasIndex0)}</Text>
      <Text>  1 in ['a','b','c']: {String(hasIndex1)}</Text>
      <Text>  5 in ['a','b','c']: {String(hasIndex5)}</Text>
      <Text>  'length' in arr: {String(hasLength)}</Text>

      <Text></Text>
      <Text>Sparse array checks:</Text>
      <Text>  0 in sparse: {String(sparseHas0)}</Text>
      <Text>  5 in sparse: {String(sparseHas5)}</Text>
      <Text>  3 in sparse (undefined): {String(sparseHas3)}</Text>
      <Text>  7 in sparse (missing): {String(sparseHas7)}</Text>

      <Text></Text>
      <Text>Optional property checks:</Text>
      <Text>  'debug' in config: {String(hasDebug)}</Text>
      <Text>  'maxItems' in config: {String(hasMaxItems)}</Text>
      <Text>  'timeout' in config: {String(hasTimeout)}</Text>

      <Text></Text>
      <Text>Generic hasOptionalProp function:</Text>
      <Text>  user has 'name': {String(userHasName)}</Text>
      <Text>  user has 'email': {String(userHasEmail)}</Text>
      <Text>  config has 'debug': {String(configHasDebug)}</Text>
    </Box>
  );
}
