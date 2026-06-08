// ink-ref-callback example — demonstrates callback refs and useRef with initial values.
//
// Callback refs pass a function instead of a ref object.
// useRef can hold any mutable value, not just DOM elements.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { useRef, useCallback, useState } from 'react';
import { Box, Text } from 'ink';

// Counter component using useRef for mutable value
function CounterWithRef() {
  const countRef = useRef<number>(0);
  const [, forceUpdate] = useState(0);

  const increment = () => {
    countRef.current++;
    forceUpdate(n => n + 1); // Trigger re-render to show updated value
  };

  return (
    <Box flexDirection="column">
      <Text>Count (via ref): {countRef.current}</Text>
      <Text onClick={increment}>[Click to increment]</Text>
    </Box>
  );
}

// Component that tracks ref assignment
function RefTracker() {
  const [assigned, setAssigned] = useState(false);
  const textRef = useRef<string>('initial');

  const callbackRef = useCallback((node: any) => {
    if (node !== null) {
      setAssigned(true);
      textRef.current = 'assigned';
    } else {
      setAssigned(false);
      textRef.current = 'cleared';
    }
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Ref assigned: {assigned ? 'yes' : 'no'}</Text>
      <Text>Ref value: {textRef.current}</Text>
    </Box>
  );
}

// Component demonstrating multiple refs
function MultiRefDemo() {
  const ref1 = useRef<number>(100);
  const ref2 = useRef<string>('hello');
  const ref3 = useRef<boolean>(true);
  const [, forceUpdate] = useState(0);

  const showValues = () => {
    forceUpdate(n => n + 1);
  };

  return (
    <Box flexDirection="column">
      <Text>ref1 (number): {ref1.current}</Text>
      <Text>ref2 (string): {ref2.current}</Text>
      <Text>ref3 (boolean): {String(ref3.current)}</Text>
      <Text onClick={showValues}>[Update display]</Text>
    </Box>
  );
}

// Component with imperative handle
function FocusInput() {
  const inputRef = useRef<{ focus: () => void; blur: () => void } | null>(null);

  const handleFocus = () => {
    inputRef.current?.focus();
  };

  const handleBlur = () => {
    inputRef.current?.blur();
  };

  // Simulated imperative handle
  const simulatedRef = useRef<{ focus: () => void; blur: () => void }>({
    focus: () => { console.log('focus'); },
    blur: () => { console.log('blur'); },
  });

  return (
    <Box flexDirection="column">
      <Text>Input ref present: {simulatedRef.current ? 'yes' : 'no'}</Text>
      <Text onClick={handleFocus}>[Simulate focus]</Text>
      <Text onClick={handleBlur}>[Simulate blur]</Text>
    </Box>
  );
}

export default function App() {
  const outerRef = useRef<HTMLDivElement | null>(null);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useRef & Callback Refs Demo</Text>
      <Text dimColor>Refs for mutable values and imperative operations</Text>
      <Text></Text>

      <Text>useRef with initial value:</Text>
      <CounterWithRef />

      <Text></Text>
      <Text>Callback ref pattern:</Text>
      <RefTracker />

      <Text></Text>
      <Text>Multiple refs of different types:</Text>
      <MultiRefDemo />

      <Text></Text>
      <Text>Imperative handle pattern:</Text>
      <FocusInput />

      <Text></Text>
      <Text>Outer container ref:</Text>
      <Text>  ref assigned: {outerRef.current ? 'yes' : 'no'}</Text>
    </Box>
  );
}
