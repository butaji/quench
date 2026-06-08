// ink-use-imperative-handle example — demonstrates useImperativeHandle and forwardRef.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: forwardRef and useImperativeHandle work in rquickjs.

import React, { useRef, useImperativeHandle, forwardRef, useState } from 'react';
import { Box, Text } from 'ink';

// --- Counter with imperative handle ---
interface CounterHandle {
  increment: () => void;
  decrement: () => void;
  reset: () => void;
  getValue: () => number;
}

const Counter = forwardRef<CounterHandle>((_props, ref) => {
  const [count, setCount] = useState(0);

  useImperativeHandle(ref, () => ({
    increment: () => setCount(c => c + 1),
    decrement: () => setCount(c => c - 1),
    reset: () => setCount(0),
    getValue: () => count,
  }), [count]);

  return (
    <Box gap={1}>
      <Text>Count:</Text>
      <Text bold color={count > 0 ? 'green' : count < 0 ? 'red' : 'white'}>
        {count}
      </Text>
    </Box>
  );
});

// --- Input with focus handle ---
interface FocusHandle {
  focus: () => void;
  blur: () => void;
}

const FocusableInput = forwardRef<FocusHandle>((_props, ref) => {
  const [focused, setFocused] = useState(false);

  useImperativeHandle(ref, () => ({
    focus: () => setFocused(true),
    blur: () => setFocused(false),
  }), [focused]);

  return (
    <Text color={focused ? 'cyan' : 'white'}>
      [{focused ? 'FOCUSED' : 'unfocused'}]
    </Text>
  );
});

// --- Timer handle ---
interface TimerHandle {
  start: () => void;
  stop: () => void;
  getElapsed: () => number;
}

const Timer = forwardRef<TimerHandle>((_props, ref) => {
  const [elapsed, setElapsed] = useState(0);
  const [running, setRunning] = useState(false);

  useImperativeHandle(ref, () => ({
    start: () => setRunning(true),
    stop: () => setRunning(false),
    getElapsed: () => elapsed,
  }), [elapsed, running]);

  return (
    <Box gap={1}>
      <Text>Timer:</Text>
      <Text color={running ? 'green' : 'gray'}>{elapsed}</Text>
      <Text dimColor>{running ? '(running)' : '(stopped)'}</Text>
    </Box>
  );
});

// --- Component to test imperative handles ---
const TestComponent = forwardRef<{ test: string }>((props, ref) => {
  useImperativeHandle(ref, () => ({
    test: 'imperative value',
  }), []);

  return <Text>Test component</Text>;
});

export default function UseImperativeHandleDemo() {
  const counterRef = useRef<CounterHandle>(null);
  const focusRef = useRef<FocusHandle>(null);
  const timerRef = useRef<TimerHandle>(null);
  const testRef = useRef<{ test: string }>(null);

  // Test refs exist (this runs after refs are set up)
  const results: string[] = [];

  if (counterRef.current) {
    counterRef.current.increment();
    counterRef.current.increment();
    results.push(`Counter initialized: ${counterRef.current.getValue()}`);
  } else {
    results.push('Counter ref: null (expected in initial render)');
  }

  if (focusRef.current) {
    focusRef.current.focus();
    results.push('Focus ref initialized');
  } else {
    results.push('Focus ref: null (expected in initial render)');
  }

  if (timerRef.current) {
    results.push(`Timer initialized: ${timerRef.current.getElapsed()}`);
  } else {
    results.push('Timer ref: null (expected in initial render)');
  }

  if (testRef.current) {
    results.push(`Test ref: ${testRef.current.test}`);
  } else {
    results.push('Test ref: null (expected in initial render)');
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useImperativeHandle & forwardRef Demo</Text>
      <Text dimColor>Exposing imperative APIs via refs</Text>
      <Text></Text>

      <Counter ref={counterRef} />
      <Box marginY={1}>
        <FocusableInput ref={focusRef} />
        <Text dimColor> Focusable input</Text>
      </Box>
      <Timer ref={timerRef} />
      <Box marginY={1}>
        <TestComponent ref={testRef} />
      </Box>

      <Text></Text>
      <Text bold>Initial State (refs set up):</Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
