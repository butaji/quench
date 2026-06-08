// ink-hoc example — demonstrates Higher-Order Components pattern.
//
// HOCs are functions that take a component and return a new enhanced component.
// They are a classic React pattern for cross-cutting concerns.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// --- withLoading HOC ---
function withLoading<P extends object>(
  WrappedComponent: React.ComponentType<P>
): React.FC<P & { isLoading: boolean }> {
  return function WithLoading({ isLoading, ...props }: P & { isLoading: boolean }) {
    if (isLoading) {
      return <Text>Loading...</Text>;
    }
    return <WrappedComponent {...props as P} />;
  };
}

// --- withCounter HOC ---
function withCounter<P extends object>(
  WrappedComponent: React.ComponentType<P & { count: number; increment: () => void }>
): React.FC<P> {
  return function WithCounter(props: P) {
    const [count, setCount] = React.useState(0);
    const increment = () => setCount(c => c + 1);
    return <WrappedComponent {...props} count={count} increment={increment} />;
  };
}

// --- withLogger HOC ---
function withLogger<P extends object>(
  WrappedComponent: React.ComponentType<P>
): React.FC<P> {
  return function WithLogger(props: P) {
    React.useEffect(() => {
      // Would log in real app
    }, []);
    return <WrappedComponent {...props} />;
  };
}

// --- withBorder HOC ---
function withBorder<P extends object>(
  WrappedComponent: React.ComponentType<P>
): React.FC<P & { border?: boolean }> {
  return function WithBorder({ border = true, ...props }: P & { border?: boolean }) {
    if (!border) {
      return <WrappedComponent {...props as P} />;
    }
    return (
      <Box borderStyle="round" padding={1}>
        <WrappedComponent {...props as P} />
      </Box>
    );
  };
}

// --- withColor HOC ---
function withColor<P extends object>(
  WrappedComponent: React.ComponentType<P>,
  color: string
): React.FC<P> {
  return function WithColor(props: P) {
    return (
      <Text color={color}>
        <WrappedComponent {...props} />
      </Text>
    );
  };
}

// --- Base components ---
function Message({ text }: { text: string }) {
  return <Text>{text}</Text>;
}

function Counter({ label, count, increment }: { label: string; count: number; increment: () => void }) {
  return (
    <Box>
      <Text>{label}: {count} </Text>
      <Text onClick={increment} color="green">[+]</Text>
    </Box>
  );
}

// --- Enhanced components via HOCs ---
const LoadingMessage = withLoading(Message);
const CountingLabel = withCounter(Counter);
const BorderedMessage = withBorder(Message);
const BlueMessage = withColor(Message, 'blue');
const GreenMessage = withColor(Message, 'green');

// --- Chained HOCs ---
const LoadingBorderedMessage = withLoading(withBorder(Message));
const CountingLoggingLabel = withLogger(withCounter(Counter));

export default function App() {
  const [showLoading, setShowLoading] = React.useState(false);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Higher-Order Components (HOC) Demo</Text>
      <Text dimColor>Functions that enhance components</Text>
      <Text></Text>

      <Text>Base component:</Text>
      <Message text="Plain message" />

      <Text></Text>
      <Text>withLoading:</Text>
      <LoadingMessage text="Data loaded" isLoading={showLoading} />

      <Text></Text>
      <Text>withCounter (click +):</Text>
      <CountingLabel label="Clicks" />

      <Text></Text>
      <Text>withBorder:</Text>
      <BorderedMessage text="Bordered text" />

      <Text></Text>
      <Text>withColor (blue):</Text>
      <BlueMessage text="Blue text" />

      <Text></Text>
      <Text>withColor (green):</Text>
      <GreenMessage text="Green text" />

      <Text></Text>
      <Text>Chained HOCs (withLoading + withBorder):</Text>
      <LoadingBorderedMessage text="Loading in border" isLoading={false} />

      <Text></Text>
      <Text>Chained HOCs (withLogger + withCounter):</Text>
      <CountingLoggingLabel label="Logged" />

      <Text></Text>
      <Text>Toggle loading: {String(showLoading)}</Text>
    </Box>
  );
}
