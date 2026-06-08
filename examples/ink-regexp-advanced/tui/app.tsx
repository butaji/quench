// ink-regexp-advanced example — demonstrates advanced RegExp features
//
// This example exercises advanced RegExp capabilities including:
// - matchAll for iterating over all matches
// - Various RegExp flags (g, i, m, s, u)
// - Split with RegExp
// - Replace with capture groups
// - test() and exec() methods
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// matchAll - iterate over all matches
const text = 'Hello World hello WORLD';
const matches = [...text.matchAll(/hello/gi)];
const matchCount = matches.length;
const firstMatch = matches[0]?.[0] || 'none';
const firstIndex = matches[0]?.index ?? -1;

// test() - quick boolean check
const hasNumber = /\d/.test('abc123');
const hasOnlyLetters = /^[a-zA-Z]+$/.test('hello');

// exec() - get match with details
const regex = /(\w+)@(\w+)\.(\w+)/;
const email = 'user@example.com';
const execResult = regex.exec(email);
const emailUser = execResult?.[1] || 'none';

// split with regex
const csv = 'apple,banana,cherry';
const fruits = csv.split(/,/);
const words = 'hello   world\t\ttab';
const wordList = words.split(/\s+/);

// replace with capture groups
const camelCase = 'hello_world_test';
const pascalCase = camelCase.replace(/_([a-z])/g, (_, c) => c.toUpperCase());

const kebabCase = 'helloWorldTest';
const snakeCase = kebabCase.replace(/[A-Z]/g, c => `_${c.toLowerCase()}`);

// match with groups
const date = '2024-01-15';
const dateParts = date.match(/(\d{4})-(\d{2})-(\d{2})/);
const year = dateParts?.[1] || 'none';
const month = dateParts?.[2] || 'none';
const day = dateParts?.[3] || 'none';

// sticky flag
const str = 'abc abc abc';
const stickyRe = /abc/y;
const firstSticky = stickyRe.exec(str)?.[0] || 'none';

// multiline flag
const multiline = 'hello\nworld\nhello';
const lineCount = (multiline.match(/^hello/gm) || []).length;

export default function RegexpDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">RegExp Advanced Demo</Text>
      <Text></Text>
      <Text>matchAll iteration:</Text>
      <Text>  matchCount: {matchCount}</Text>
      <Text>  firstMatch: {firstMatch}</Text>
      <Text>  firstIndex: {firstIndex}</Text>
      <Text></Text>
      <Text>test() method:</Text>
      <Text>  hasNumber: {hasNumber ? 'true' : 'false'}</Text>
      <Text>  hasOnlyLetters: {hasOnlyLetters ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>exec() with groups:</Text>
      <Text>  email: {email}</Text>
      <Text>  emailUser: {emailUser}</Text>
      <Text></Text>
      <Text>split with regex:</Text>
      <Text>  fruits: {fruits.join(', ')}</Text>
      <Text>  words: {wordList.join(', ')}</Text>
      <Text></Text>
      <Text>replace with capture groups:</Text>
      <Text>  camelCase: {camelCase}</Text>
      <Text>  pascalCase: {pascalCase}</Text>
      <Text>  kebabCase: {kebabCase}</Text>
      <Text>  snakeCase: {snakeCase}</Text>
      <Text></Text>
      <Text>date parsing:</Text>
      <Text>  year: {year}, month: {month}, day: {day}</Text>
      <Text></Text>
      <Text>multiline flag:</Text>
      <Text>  lineCount: {lineCount}</Text>
    </Box>
  );
}
