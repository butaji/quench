// ink-date-math example — Date, Math, Intl
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Test Date and Math APIs
export default function App() {
  const now = new Date(2024, 0, 15, 14, 30, 0);
  const timestamp = Date.now();
  const iso = now.toISOString();
  const locale = now.toLocaleDateString('en-US', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' });
  const timeStr = now.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });

  // Math constants and methods
  const pi = Math.PI;
  const e = Math.E;
  const rand = Math.random();
  const abs = Math.abs(-42);
  const floor = Math.floor(4.7);
  const ceil = Math.ceil(4.2);
  const round = Math.round(4.5);
  const max = Math.max(1, 5, 3, 9, 2);
  const min = Math.min(1, 5, 3, 9, 2);
  const sinVal = Math.sin(pi / 2);

  return (
    <Box flexDirection="column">
      <Text>=== Date ===</Text>
      <Text>Date.now: {timestamp}</Text>
      <Text>toISOString: {iso}</Text>
      <Text>toLocaleDateString: {locale}</Text>
      <Text>toLocaleTimeString: {timeStr}</Text>
      <Text>getFullYear: {now.getFullYear()}</Text>
      <Text>getMonth: {now.getMonth()}</Text>
      <Text>getDate: {now.getDate()}</Text>
      <Text>getHours: {now.getHours()}</Text>
      <Text>getMinutes: {now.getMinutes()}</Text>
      <Text>getSeconds: {now.getSeconds()}</Text>
      <Text>getDay: {now.getDay()}</Text>
      <Text>valueOf: {now.valueOf()}</Text>
      <Text>=== Math ===</Text>
      <Text>Math.PI: {pi}</Text>
      <Text>Math.E: {e}</Text>
      <Text>Math.random: {rand}</Text>
      <Text>Math.abs(-42): {abs}</Text>
      <Text>Math.floor(4.7): {floor}</Text>
      <Text>Math.ceil(4.2): {ceil}</Text>
      <Text>Math.round(4.5): {round}</Text>
      <Text>Math.max(1,5,3,9,2): {max}</Text>
      <Text>Math.min(1,5,3,9,2): {min}</Text>
      <Text>Math.sin(PI/2): {sinVal}</Text>
    </Box>
  );
}
