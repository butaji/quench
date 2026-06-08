// ink-regexp-test-exec example — demonstrates RegExp test and exec methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: RegExp methods are standard JavaScript runtime features.

import React from 'react';
import { Box, Text } from 'ink';

// --- test method ---
const re1 = /hello/i;
const hasHello1 = re1.test('Hello World');
const hasHello2 = re1.test('Goodbye World');
const re2 = /\d+/;
const hasDigits = re2.test('abc123');

// --- exec method ---
const re3 = /(\w+)\s+(\w+)/;
const match1 = re3.exec('Hello World');
const re4 = /\d+/g;
const match2 = re4.exec('abc123def456');
const match3 = re4.exec('abc123def456'); // second call continues from last index

// --- lastIndex property ---
const re5 = /\d+/g;
const lastMatch1 = re5.exec('abc123def456');
const lastMatch2 = re5.exec('abc789'); // should start from lastIndex

// --- match method ---
const str1 = 'Hello World';
const match4 = str1.match(/o/);
const match5 = str1.match(/o/g);
const match6 = str1.match(/x/);

// --- replace ---
const str2 = 'hello world';
const replaced = str2.replace(/world/, 'there');
const replacedAll = 'foo foo foo'.replace(/foo/g, 'bar');

// --- search ---
const str3 = 'hello world';
const idx1 = str3.search(/world/);
const idx2 = str3.search(/xyz/);

// --- split ---
const str4 = 'a,b,c,d';
const parts1 = str4.split(',');
const parts2 = 'hello world'.split(/\s+/);

export default function RegExpTestExecDemo() {
  const results: string[] = [];

  // test
  results.push(`re: /hello/i`);
  results.push(`test('Hello World'): ${hasHello1}`);
  results.push(`test('Goodbye World'): ${hasHello2}`);
  results.push(`re: /\\d+/`);
  results.push(`test('abc123'): ${hasDigits}`);

  results.push('');

  // exec
  results.push(`re: /(\\w+)\\s+(\\w+)/`);
  results.push(`exec('Hello World'): ${match1 ? match1[0] : 'null'}`);
  results.push(`  groups: ${match1 ? match1[1] + ', ' + match1[2] : 'null'}`);

  results.push('');

  // match
  results.push(`'Hello World'.match(/o/): ${match4 ? match4[0] : 'null'}`);
  results.push(`'Hello World'.match(/o/g): [${match5 ? match5.join(', ') : 'null'}]`);
  results.push(`'Hello World'.match(/x/): ${match6 ? match6 : 'null'}`);

  results.push('');

  // replace
  results.push(`'hello world'.replace(/world/, 'there'): ${replaced}`);
  results.push(`'foo foo foo'.replace(/foo/g, 'bar'): ${replacedAll}`);

  results.push('');

  // search
  results.push(`'hello world'.search(/world/): ${idx1}`);
  results.push(`'hello world'.search(/xyz/): ${idx2}`);

  results.push('');

  // split
  results.push(`'a,b,c,d'.split(','): [${parts1.join(', ')}]`);
  results.push(`'hello world'.split(/\\s+/): [${parts2.join(', ')}]`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">RegExp Test/Exec Demo</Text>
      <Text dimColor>RegExp test, exec, match, replace, search, split</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
