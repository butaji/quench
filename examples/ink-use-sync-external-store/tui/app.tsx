// ink-use-sync-external-store example — demonstrates React 18 concurrent hooks.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: useSyncExternalStore and useDeferredValue work in rquickjs.

import React, { useSyncExternalStore, useDeferredValue, useState } from 'react';
import { Box, Text } from 'ink';

// --- External store for terminal dimensions ---
interface StoreState {
  width: number;
  height: number;
  theme: 'light' | 'dark';
}

let storeState: StoreState = {
  width: 80,
  height: 24,
  theme: 'light',
};

const storeListeners = new Set<() => void>();

const externalStore = {
  subscribe: (callback: () => void): (() => void) => {
    storeListeners.add(callback);
    return () => storeListeners.delete(callback);
  },
  getSnapshot: (): StoreState => storeState,
  getServerSnapshot: (): StoreState => ({ width: 80, height: 24, theme: 'light' }),
  setState: (partial: Partial<StoreState>) => {
    storeState = { ...storeState, ...partial };
    storeListeners.forEach(cb => cb());
  },
};

// --- Counter store with atomic operations ---
let counterValue = 0;
const counterListeners = new Set<() => void>();

const counterStore = {
  subscribe: (callback: () => void): (() => void) => {
    counterListeners.add(callback);
    return () => counterListeners.delete(callback);
  },
  getSnapshot: (): number => counterValue,
  getServerSnapshot: (): number => 0,
  increment: () => {
    counterValue++;
    counterListeners.forEach(cb => cb());
  },
  decrement: () => {
    counterValue--;
    counterListeners.forEach(cb => cb());
  },
  reset: () => {
    counterValue = 0;
    counterListeners.forEach(cb => cb());
  },
};

// --- Component using useSyncExternalStore ---
function StoreReader({ label }: { label: string }) {
  const store = useSyncExternalStore(
    externalStore.subscribe,
    externalStore.getSnapshot,
    externalStore.getServerSnapshot
  );

  return (
    <Text>
      {label}: {store.width}x{store.height}, theme={store.theme}
    </Text>
  );
}

// --- Component using useSyncExternalStore for counter ---
function CounterReader() {
  const count = useSyncExternalStore(
    counterStore.subscribe,
    counterStore.getSnapshot,
    counterStore.getServerSnapshot
  );

  return (
    <Text color={count > 0 ? 'green' : count < 0 ? 'red' : 'white'}>
      Counter: {count}
    </Text>
  );
}

// --- Component using useDeferredValue ---
function DeferredInput() {
  const [input, setInput] = useState('initial text');
  const deferredInput = useDeferredValue(input);

  return (
    <Box flexDirection="column">
      <Text dimColor>Input: {input}</Text>
      <Text dimColor>Deferred: {deferredInput}</Text>
    </Box>
  );
}

// --- Multiple deferred values ---
function DeferredList() {
  const [filter, setFilter] = useState('all');
  const deferredFilter = useDeferredValue(filter);

  const items = [
    { id: 1, name: 'Apple', category: 'fruit' },
    { id: 2, name: 'Carrot', category: 'veg' },
    { id: 3, name: 'Banana', category: 'fruit' },
    { id: 4, name: 'Broccoli', category: 'veg' },
  ];

  const filtered = deferredFilter === 'all'
    ? items
    : items.filter(i => i.category === deferredFilter);

  return (
    <Box flexDirection="column">
      <Text dimColor>Filter: {filter} (deferred: {deferredFilter})</Text>
      {filtered.map(item => (
        <Text key={item.id}>{item.name}</Text>
      ))}
    </Box>
  );
}

export default function UseSyncExternalStoreDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useSyncExternalStore & useDeferredValue Demo</Text>
      <Text dimColor>React 18 concurrent hooks</Text>
      <Text></Text>

      <Text bold>Store Values:</Text>
      <StoreReader label="Terminal" />
      <CounterReader />

      <Text></Text>
      <Text bold>Deferred Values:</Text>
      <DeferredInput />

      <Text></Text>
      <Text bold>Deferred List (filter='all'):</Text>
      <DeferredList />

      <Text></Text>
      <Text dimColor italic>
        Note: useDeferredValue returns the same value in sync context.
        It defers updates during renders for non-urgent UI.
      </Text>
    </Box>
  );
}
