// ink-render-props example — demonstrates render props pattern.
//
// Render props is a pattern where a component receives a function prop
// that it calls with data, allowing parent components to control rendering.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { useState } from 'react';
import { Box, Text } from 'ink';

// --- Mouse Tracker with render props ---
interface MouseTrackerProps {
  x: number;
  y: number;
  render: (pos: { x: number; y: number }) => React.ReactNode;
}

function MouseTracker({ x, y, render }: MouseTrackerProps) {
  return <>{render({ x, y })}</>;
}

// --- List with render props ---
interface ListProps<T> {
  items: T[];
  renderItem: (item: T, index: number) => React.ReactNode;
}

function List<T>({ items, renderItem }: ListProps<T>) {
  return (
    <Box flexDirection="column">
      {items.map((item, index) => (
        <Box key={index}>{renderItem(item, index)}</Box>
      ))}
    </Box>
  );
}

// --- Data Provider with render props ---
interface DataProviderProps {
  data: { name: string; value: number }[];
  render: (data: { name: string; value: number }[]) => React.ReactNode;
}

function DataProvider({ data, render }: DataProviderProps) {
  return <>{render(data)}</>;
}

// --- Counter with render props ---
interface CounterProps {
  initial: number;
  render: (count: number, increment: () => void) => React.ReactNode;
}

function Counter({ initial, render }: CounterProps) {
  const [count, setCount] = useState(initial);
  const increment = () => setCount(c => c + 1);
  return <>{render(count, increment)}</>;
}

export default function App() {
  const mousePos = { x: 42, y: 13 };
  const data = [
    { name: 'Alpha', value: 100 },
    { name: 'Beta', value: 200 },
    { name: 'Gamma', value: 300 },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Render Props Pattern Demo</Text>
      <Text dimColor>Component receives function to control rendering</Text>
      <Text></Text>

      <Text>Mouse Tracker:</Text>
      <MouseTracker x={mousePos.x} y={mousePos.y} render={({ x, y }) => (
        <Text>  Position: ({x}, {y})</Text>
      )} />

      <Text></Text>
      <Text>List with render props:</Text>
      <List items={['React', 'Vue', 'Angular']} renderItem={(item, i) => (
        <Text>  {i + 1}. {item}</Text>
      )} />

      <Text></Text>
      <Text>Data Provider:</Text>
      <DataProvider data={data} render={(items) => (
        <Box flexDirection="column">
          {items.map((d, i) => (
            <Text key={i}>  {d.name}: {d.value}</Text>
          ))}
        </Box>
      )} />

      <Text></Text>
      <Text>Counter (click to increment):</Text>
      <Counter initial={0} render={(count, increment) => (
        <Text>  Count: {count} [render props provides increment]</Text>
      )} />
    </Box>
  );
}
