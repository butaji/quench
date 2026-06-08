// ink-object-entries-values example — demonstrates Object.entries, Object.values, Object.keys
//
// This example exercises the static Object methods for working with object
// enumerable properties:
// - Object.keys(obj) - returns array of own enumerable property names
// - Object.values(obj) - returns array of own enumerable property values
// - Object.entries(obj) - returns array of [key, value] pairs
// - Object.fromEntries(pairs) - converts entries back to object
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Sample object for demonstration
const user = {
  name: 'Alice',
  age: 30,
  city: 'New York'
};

// Object.keys - get property names
const userKeys = Object.keys(user);

// Object.values - get property values
const userValues = Object.values(user);

// Object.entries - get [key, value] pairs
const userEntries = Object.entries(user);

// Object.fromEntries - convert entries back to object
const entryPairs = [['a', 1], ['b', 2], ['c', 3]];
const fromEntries = Object.fromEntries(entryPairs);

// Nested object entries
const nested = {
  level1: {
    key1: 'value1',
    key2: 'value2'
  },
  level2: 'simple'
};
const nestedEntries = Object.entries(nested);

// Count properties with values
const scores = { math: 95, science: 88, history: 92 };
const scoreEntries = Object.entries(scores);
const totalScore = Object.values(scores).reduce((sum, v) => sum + v, 0);
const avgScore = totalScore / Object.keys(scores).length;

// Filter entries by value - using a function to avoid JSX issues
function filterHighScores(entries: [string, number][]): [string, number][] {
  return entries.filter(([, score]) => score >= 90);
}

// Transform entries
const uppercasedEntries = Object.entries(user).map(([k, v]) => [k, String(v).toUpperCase()]);

// fromEntries with Map
const mapEntries = new Map([['x', 10], ['y', 20]]);
const fromMap = Object.fromEntries(mapEntries);

// Empty object handling
const emptyObj = {};
const emptyKeys = Object.keys(emptyObj);
const emptyValues = Object.values(emptyObj);
const emptyEntries = Object.entries(emptyObj);

// Array-like object (sparse)
const arrayLike = { 0: 'a', 1: 'b', 2: 'c', length: 3 };
const arrayLikeKeys = Object.keys(arrayLike);
const arrayLikeValues = Object.values(arrayLike);

// Pre-compute filtered entries
const filteredEntries = filterHighScores(scoreEntries);

// Format entries for display
function formatEntries(entries: [string, unknown][]): string {
  return entries.map(([k, v]) => `${k}:${v}`).join(', ');
}

export default function ObjectEntriesValues() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Object.keys/values/entries</Text>
      <Text></Text>
      <Text>user object:</Text>
      <Text>  keys: [{userKeys.join(', ')}]</Text>
      <Text>  values: [{userValues.join(', ')}]</Text>
      <Text>  entries: [{formatEntries(userEntries)}]</Text>
      <Text></Text>
      <Text>fromEntries([[a,1],[b,2],[c,3]]):</Text>
      <Text>  {JSON.stringify(fromEntries)}</Text>
      <Text></Text>
      <Text>Score analysis:</Text>
      <Text>  entries: [{formatEntries(scoreEntries)}]</Text>
      <Text>  total: {totalScore}, avg: {avgScore}</Text>
      <Text>  filtered (score &gt;= 90): [{formatEntries(filteredEntries)}]</Text>
      <Text></Text>
      <Text>Transform entries:</Text>
      <Text>  {JSON.stringify(Object.fromEntries(uppercasedEntries))}</Text>
      <Text></Text>
      <Text>fromEntries with Map:</Text>
      <Text>  {JSON.stringify(fromMap)}</Text>
      <Text></Text>
      <Text>Empty object:</Text>
      <Text>  keys: [{emptyKeys.join(', ')}], values: [{emptyValues.join(', ')}]</Text>
      <Text></Text>
      <Text>Array-like object:</Text>
      <Text>  keys: [{arrayLikeKeys.join(', ')}]</Text>
      <Text>  values: [{arrayLikeValues.join(', ')}]</Text>
    </Box>
  );
}
