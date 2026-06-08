// ink-queue-microtask example — demonstrates queueMicrotask.
//
// queueMicrotask queues a callback to run asynchronously after the current
// synchronous code but before other async tasks (like setTimeout).
//
// This example shows the execution order of synchronous code vs microtasks.

import React from 'react';
import { Box, Text } from 'ink';

// Track execution order - this runs synchronously during module evaluation
const executionOrder: string[] = [];

// Synchronous code runs first
executionOrder.push('sync-start');

// queueMicrotask schedules callback to run after sync code
queueMicrotask(() => {
  executionOrder.push('microtask-1');
});

// Another sync code
executionOrder.push('sync-middle');

// Promise callbacks also run as microtasks (after current sync code)
Promise.resolve().then(() => {
  executionOrder.push('promise-then');
});

// Another microtask
queueMicrotask(() => {
  executionOrder.push('microtask-2');
});

// Final sync code
executionOrder.push('sync-end');

export default function QueueMicrotaskDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">queueMicrotask Demo</Text>
      <Text dimColor>Microtask queue execution order</Text>
      <Text></Text>
      <Text>Synchronous code (module evaluation order):</Text>
      {executionOrder.map((item, i) => (
        <Text key={i}>{i + 1}. {item}</Text>
      ))}
      <Text></Text>
      <Text dimColor>Note: Microtasks and Promise callbacks are queued</Text>
      <Text dimColor>but don't appear here as they run after component renders.</Text>
    </Box>
  );
}
