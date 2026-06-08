// ink-date-comprehensive example — demonstrates comprehensive Date methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: toLocaleString/tocaleTimeString may differ between environments.

import React from 'react';
import { Box, Text } from 'ink';

// Create a fixed date for consistent output: March 15, 2024, 14:30:45 UTC
const date = new Date(Date.UTC(2024, 2, 15, 14, 30, 45, 123));

// Get methods
const year = date.getFullYear();
const month = date.getMonth(); // 0-indexed
const day = date.getDate();
const hours = date.getHours();
const minutes = date.getMinutes();
const seconds = date.getSeconds();
const ms = date.getMilliseconds();
const dayOfWeek = date.getDay();

// UTC variants
const utcYear = date.getUTCFullYear();
const utcMonth = date.getUTCMonth();
const utcDay = date.getUTCDate();
const utcHours = date.getUTCHours();
const utcMinutes = date.getUTCMinutes();
const utcSeconds = date.getUTCSeconds();

// Time values
const timestamp = date.getTime();

// toString variants
const isoStr = date.toISOString();
const dateStr = date.toDateString();

// Math with dates
const tomorrow = new Date(date.getTime() + 86400000);
const yesterday = new Date(date.getTime() - 86400000);

export default function DateComprehensiveDemo() {
  const results: string[] = [];

  // Basic getters
  results.push(`Date: ${isoStr}`);
  results.push(`getFullYear(): ${year}`);
  results.push(`getMonth(): ${month} (0=Jan)`);
  results.push(`getDate(): ${day}`);
  results.push(`getHours(): ${hours}`);
  results.push(`getMinutes(): ${minutes}`);
  results.push(`getSeconds(): ${seconds}`);
  results.push(`getMilliseconds(): ${ms}`);
  results.push(`getDay(): ${dayOfWeek} (0=Sun)`);

  results.push('');

  // UTC getters
  results.push(`getUTCFullYear(): ${utcYear}`);
  results.push(`getUTCMonth(): ${utcMonth}`);
  results.push(`getUTCDate(): ${utcDay}`);
  results.push(`getUTCHours(): ${utcHours}`);
  results.push(`getUTCMinutes(): ${utcMinutes}`);
  results.push(`getUTCSeconds(): ${utcSeconds}`);

  results.push('');

  // Time value
  results.push(`getTime(): ${timestamp}`);

  results.push('');

  // String representations
  results.push(`toISOString(): ${isoStr}`);
  results.push(`toDateString(): ${dateStr}`);

  results.push('');

  // Date math
  results.push(`tomorrow UTC: ${tomorrow.toISOString()}`);
  results.push(`yesterday UTC: ${yesterday.toISOString()}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Date Comprehensive Demo</Text>
      <Text dimColor>Core Date methods (UTC-based for consistency)</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
