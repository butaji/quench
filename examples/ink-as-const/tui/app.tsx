// ink-as-const example — demonstrates `as const`, literal types, and tuple types.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: `as const`, literal types, and tuple types are erased at compile time.
// They have no runtime impact on the generated JavaScript or Rust code.

import React from 'react';
import { Box, Text } from 'ink';

// --- as const with arrays ---
const COLORS = ['red', 'green', 'blue'] as const;
type Color = typeof COLORS[number];

// --- as const with objects ---
const CONFIG = {
  title: 'My App',
  version: 1,
  enabled: true,
  features: ['auth', 'billing'],
} as const;
type Config = typeof CONFIG;

// --- Literal type inference ---
const DIRECTIONS = ['north', 'south', 'east', 'west'] as const;
type Direction = typeof DIRECTIONS[number];

// --- Numeric literal types ---
const HTTP_CODES = {
  OK: 200,
  NOT_FOUND: 404,
  SERVER_ERROR: 500,
} as const;
type HttpCode = typeof HTTP_CODES[keyof typeof HTTP_CODES];

// --- Labeled tuple types ---
type Point = [x: number, y: number];
const origin: Point = [0, 0];
const point: Point = [10, 20];

// --- Optional tuple elements ---
type OptionalTuple = [string, number?, boolean?];
const opt1: OptionalTuple = ['required'];
const opt2: OptionalTuple = ['required', 42];

// --- Status union from const array ---
const STATUS = ['idle', 'loading', 'done', 'error'] as const;
type Status = typeof STATUS[number];
const currentStatus: Status = 'loading';

// --- Nested as const ---
const NESTED = {
  db: {
    host: 'localhost',
    port: 5432,
  },
  cache: {
    host: 'redis.local',
    port: 6379,
  },
} as const;

// --- Function returning literal type ---
function getDefaultStatus(): Status {
  return 'idle';
}

// --- Template literal types ---
type HexColor = '#ff0000' | '#00ff00' | '#0000ff';
const validColor: HexColor = '#ff0000';

export default function AsConstDemo() {
  const results: string[] = [];

  // Colors
  results.push(`Colors[0]: ${COLORS[0]}`);
  results.push(`Colors count: ${COLORS.length}`);
  results.push(`Color type: ${typeof COLORS[0]}`);

  // Config
  results.push(`Config.title: ${CONFIG.title}`);
  results.push(`Config.version: ${CONFIG.version}`);
  results.push(`Config.features: ${CONFIG.features.join(', ')}`);
  results.push(`Config.enabled: ${CONFIG.enabled}`);

  // Directions
  for (const dir of DIRECTIONS) {
    results.push(`Direction: ${dir}`);
  }

  // HTTP codes
  results.push(`HTTP OK: ${HTTP_CODES.OK}`);
  results.push(`HTTP NOT_FOUND: ${HTTP_CODES.NOT_FOUND}`);
  results.push(`HTTP SERVER_ERROR: ${HTTP_CODES.SERVER_ERROR}`);

  // Points
  results.push(`Origin: (${origin[0]}, ${origin[1]})`);
  results.push(`Point: (${point[0]}, ${point[1]})`);

  // Optional tuples
  results.push(`Optional1: ${opt1[0]}`);
  results.push(`Optional2: ${opt2[0]}, ${opt2[1] ?? 'undefined'}`);

  // Status
  results.push(`Current status: ${currentStatus}`);
  results.push(`Default status: ${getDefaultStatus()}`);

  // Nested
  results.push(`DB host: ${NESTED.db.host}`);
  results.push(`Cache port: ${NESTED.cache.port}`);

  // Template literal type (runtime value)
  results.push(`Valid color: ${validColor}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">as const, Literal Types & Tuples Demo</Text>
      <Text dimColor>All type assertions erased at compile time</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
