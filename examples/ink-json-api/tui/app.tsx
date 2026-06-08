// ink-json-api example — demonstrates JSON.stringify and JSON.parse.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: JSON is a standard JavaScript global object.

import React, { useState } from 'react';
import { Box, Text } from 'ink';

// --- Basic stringify and parse ---
const config = {
  app: 'MyApp',
  version: '1.0.0',
  debug: true,
  port: 8080,
};
const jsonStr = JSON.stringify(config);
const reparsed = JSON.parse(jsonStr);

// --- Pretty print with indentation ---
const prettyJson = JSON.stringify(config, null, 2);

// --- Selective keys ---
const selected = JSON.stringify(config, ['app', 'version']);

// --- With replacer function ---
const withReplacer = JSON.stringify(config, (key, value) => {
  if (typeof value === 'number') {
    return value * 2;
  }
  return value;
});

// --- Parse with error handling ---
function safeParse(text: string): { success: boolean; data?: any; error?: string } {
  try {
    const parsed = JSON.parse(text);
    return { success: true, data: parsed };
  } catch (e: any) {
    return { success: false, error: e.message };
  }
}

const invalidJson = '{ invalid json }';
const validJson = '{"key": "value", "num": 42}';

const parseResult1 = safeParse(invalidJson);
const parseResult2 = safeParse(validJson);

// --- Array parsing ---
const jsonArray = '[1, 2, 3, 4, 5]';
const parsedArray = JSON.parse(jsonArray);

// --- Nested object ---
const nested = {
  user: {
    name: 'Alice',
    address: {
      city: 'NYC',
      zip: '10001',
    },
  },
  tags: ['admin', 'active'],
};
const nestedJson = JSON.stringify(nested);
const parsedNested = JSON.parse(nestedJson);

// --- JSON.stringify with array of primitives ---
const numbers = [1, 2, 3, 4, 5];
const numbersJson = JSON.stringify(numbers);

export default function JsonApiDemo() {
  const [input, setInput] = useState('{"name":"Test"}');
  const [dynamicResult, setDynamicResult] = useState(safeParse(input));

  const results: string[] = [];

  // Basic operations
  results.push(`Original: app=${config.app}, version=${config.version}`);
  results.push(`Stringified: ${jsonStr}`);
  results.push(`Reparsed: ${reparsed.debug}`);

  // Pretty print (truncated for display)
  results.push(`Pretty (truncated): ${prettyJson.split('\n').slice(0, 2).join(' | ')}`);

  // Selective keys
  results.push(`Selective keys: ${selected}`);

  // With replacer
  results.push(`With replacer (doubled): port=${JSON.parse(JSON.stringify({ port: 8080 }, (k, v) => typeof v === 'number' ? v * 2 : v)).port}`);

  // Safe parse results
  results.push(`Parse invalid: success=${parseResult1.success}, error=${parseResult1.error ? 'yes' : 'no'}`);
  results.push(`Parse valid: success=${parseResult2.success}, data=${JSON.stringify(parseResult2.data)}`);

  // Array parsing
  results.push(`Parsed array: [${parsedArray.join(', ')}]`);

  // Nested object
  results.push(`Nested.city: ${parsedNested.user.address.city}`);
  results.push(`Nested.tags: ${parsedNested.tags.join(', ')}`);

  // Numbers array
  results.push(`Numbers array: ${numbersJson}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">JSON.stringify & JSON.parse Demo</Text>
      <Text dimColor>Standard JavaScript JSON API</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
