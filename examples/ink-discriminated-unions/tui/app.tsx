// ink-discriminated-unions example — demonstrates discriminated unions
//
// Discriminated unions (tagged unions) are a powerful TypeScript pattern
// where each member of a union has a common 'discriminant' property
// with a unique literal type. This enables exhaustive type checking.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Event discriminated union
type Event =
  | { type: 'click'; x: number; y: number }
  | { type: 'keypress'; key: string }
  | { type: 'resize'; width: number; height: number };

function handleEvent(e: Event): string {
  switch (e.type) {
    case 'click':
      return `Click at (${e.x}, ${e.y})`;
    case 'keypress':
      return `Key: ${e.key}`;
    case 'resize':
      return `Resized to ${e.width}x${e.height}`;
    default:
      const _exhaustive: never = e;
      return _exhaustive;
  }
}

// Shape discriminated union
type Shape =
  | { kind: 'circle'; radius: number }
  | { kind: 'rect'; width: number; height: number }
  | { kind: 'triangle'; base: number; height: number };

function area(s: Shape): number {
  switch (s.kind) {
    case 'circle':
      return Math.PI * s.radius * s.radius;
    case 'rect':
      return s.width * s.height;
    case 'triangle':
      return (s.base * s.height) / 2;
    default:
      const _e: never = s;
      return _e;
  }
}

function perimeter(s: Shape): number {
  switch (s.kind) {
    case 'circle':
      return 2 * Math.PI * s.radius;
    case 'rect':
      return 2 * (s.width + s.height);
    case 'triangle':
      return 3; // Assuming equilateral for simplicity
    default:
      const _e: never = s;
      return _e;
  }
}

// Result discriminated union (like Rust's Result)
type Result<T, E> =
  | { success: true; value: T }
  | { success: false; error: E };

function divide(a: number, b: number): Result<number, string> {
  if (b === 0) {
    return { success: false, error: 'Division by zero' };
  }
  return { success: true, value: a / b };
}

// Optional discriminated union
type Optional<T> =
  | { defined: true; value: T }
  | { defined: false };

function getValue<T>(opt: Optional<T>, fallback: T): T {
  return opt.defined ? opt.value : fallback;
}

// Async state discriminated union
type AsyncState<T> =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; data: T }
  | { status: 'error'; message: string };

const loading: AsyncState<string> = { status: 'loading' };
const success: AsyncState<number> = { status: 'success', data: 42 };
const error: AsyncState<number> = { status: 'error', message: 'Network error' };

export default function DiscriminatedUnionsDemo() {
  const events: Event[] = [
    { type: 'click', x: 10, y: 20 },
    { type: 'keypress', key: 'Enter' },
    { type: 'resize', width: 800, height: 600 },
  ];

  const shapes: Shape[] = [
    { kind: 'circle', radius: 5 },
    { kind: 'rect', width: 4, height: 3 },
    { kind: 'triangle', base: 6, height: 4 },
  ];

  const div1 = divide(10, 2);
  const div2 = divide(5, 0);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Discriminated Unions Demo</Text>
      <Text></Text>
      <Text>Event handling:</Text>
      {events.map((e, i) => (
        <Text key={i}>  {handleEvent(e)}</Text>
      ))}
      <Text></Text>
      <Text>Shape areas:</Text>
      {shapes.map((s, i) => (
        <Text key={i}>  {s.kind}: {area(s).toFixed(2)}</Text>
      ))}
      <Text></Text>
      <Text>Shape perimeters:</Text>
      {shapes.map((s, i) => (
        <Text key={i}>  {s.kind}: {perimeter(s).toFixed(2)}</Text>
      ))}
      <Text></Text>
      <Text>Result pattern:</Text>
      <Text>  div(10, 2): {div1.success ? div1.value.toString() : div1.error}</Text>
      <Text>  div(5, 0): {div2.success ? div2.value.toString() : div2.error}</Text>
      <Text></Text>
      <Text>Optional pattern:</Text>
      <Text>  with value: {getValue({ defined: true, value: 42 }, 0)}</Text>
      <Text>  without value: {getValue({ defined: false }, 100)}</Text>
      <Text></Text>
      <Text>AsyncState pattern:</Text>
      <Text>  loading: {loading.status}</Text>
      <Text>  success: data={success.data}</Text>
      <Text>  error: {error.message}</Text>
    </Box>
  );
}
