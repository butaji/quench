// Component Composition — TuiBridge
// Demonstrates real-world component patterns: reusable UI components
// Shows how to build larger apps from small, composable pieces

import { render, Box, Text, useState, useInput, useApp } from 'ink';

// ============================================================================
// Reusable Components
// ============================================================================

interface ButtonProps {
  label: string;
  onPress: () => void;
  variant?: 'default' | 'primary' | 'danger';
  disabled?: boolean;
}

function Button({ label, onPress, variant = 'default', disabled = false }: ButtonProps): JSX.Element {
  const [focused, setFocused] = useState(false);

  const colors = {
    default: { fg: 'white', bg: '#333' },
    primary: { fg: 'black', bg: 'cyan' },
    danger: { fg: 'white', bg: 'red' },
  };

  const { fg, bg } = colors[variant];
  const borderColor = disabled ? 'gray' : (focused ? 'yellow' : fg);

  return (
    <Box
      borderStyle={focused ? 'round' : 'single'}
      borderColor={borderColor}
      backgroundColor={disabled ? undefined : bg}
      paddingX={2}
      paddingY={1}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
    >
      <Text color={disabled ? 'gray' : fg} dimColor={disabled}>
        {focused ? '► ' : '  '}{label}{focused ? ' ◄' : ''}
      </Text>
    </Box>
  );
}

interface CardProps {
  title: string;
  children: JSX.Element | JSX.Element[];
  borderColor?: string;
}

function Card({ title, children, borderColor = 'cyan' }: CardProps): JSX.Element {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor={borderColor}>
      <Box backgroundColor={borderColor} paddingX={1}>
        <Text color="black" bold>{title}</Text>
      </Box>
      <Box flexDirection="column" padding={1}>
        {children}
      </Box>
    </Box>
  );
}

interface StatusBadgeProps {
  status: 'success' | 'warning' | 'error' | 'info';
  label: string;
}

function StatusBadge({ status, label }: StatusBadgeProps): JSX.Element {
  const colors = {
    success: { bg: 'green', text: 'black' },
    warning: { bg: 'yellow', text: 'black' },
    error: { bg: 'red', text: 'white' },
    info: { bg: 'cyan', text: 'black' },
  };

  const { bg, text } = colors[status];

  return (
    <Box backgroundColor={bg} paddingX={1}>
      <Text color={text} bold>{label}</Text>
    </Box>
  );
}

interface SpinnerProps {
  size?: number;
}

function Spinner({ size = 4 }: SpinnerProps): JSX.Element {
  const [frame, setFrame] = useState(0);
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

  useState(() => {
    const interval = setInterval(() => {
      setFrame(f => (f + 1) % frames.length);
    }, 80);
    return () => clearInterval(interval);
  });

  return <Text color="cyan">{frames[frame]}</Text>;
}

// ============================================================================
// Main App
// ============================================================================

function App(): JSX.Element {
  const [activeTab, setActiveTab] = useState<'home' | 'settings' | 'about'>('home');

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === '1') setActiveTab('home');
    if (input === '2') setActiveTab('settings');
    if (input === '3') setActiveTab('about');
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      {/* Header */}
      <Box flexDirection="row" justifyContent="space-between" marginBottom={1}>
        <Text bold color="green">My CLI App</Text>
        <Box flexDirection="row" gap={1}>
          <Text color={activeTab === 'home' ? 'cyan' : 'gray'}>[1] Home</Text>
          <Text color={activeTab === 'settings' ? 'cyan' : 'gray'}>[2] Settings</Text>
          <Text color={activeTab === 'about' ? 'cyan' : 'gray'}>[3] About</Text>
        </Box>
      </Box>

      {/* Content */}
      {activeTab === 'home' && (
        <Box flexDirection="column" gap={1}>
          <Card title="Dashboard">
            <Box flexDirection="row" gap={2}>
              <StatusBadge status="success" label="Online" />
              <StatusBadge status="info" label="v1.0.0" />
            </Box>
            <Text> </Text>
            <Box flexDirection="row">
              <Text dimColor>Loading: </Text>
              <Spinner />
            </Box>
          </Card>

          <Card title="Quick Actions" borderColor="green">
            <Box flexDirection="row" gap={1}>
              <Button label="Start" variant="primary" onPress={() => {}} />
              <Button label="Stop" variant="danger" onPress={() => {}} />
              <Button label="Restart" onPress={() => {}} />
            </Box>
          </Card>
        </Box>
      )}

      {activeTab === 'settings' && (
        <Card title="Settings" borderColor="yellow">
          <Box flexDirection="row" justifyContent="space-between" width={50}>
            <Text>Theme:</Text>
            <Text color="cyan">Dark</Text>
          </Box>
          <Box flexDirection="row" justifyContent="space-between" width={50}>
            <Text>Auto-save:</Text>
            <Text color="green">Enabled</Text>
          </Box>
          <Box flexDirection="row" justifyContent="space-between" width={50}>
            <Text>Notifications:</Text>
            <Text color="green">Enabled</Text>
          </Box>
        </Card>
      )}

      {activeTab === 'about' && (
        <Card title="About" borderColor="magenta">
          <Text>My CLI App</Text>
          <Text dimColor>Version 1.0.0</Text>
          <Text> </Text>
          <Text dimColor small>Built with React + Ink + TuiBridge</Text>
        </Card>
      )}

      <Text> </Text>
      <Text dimColor>[1-3] switch tab | [q] quit</Text>
    </Box>
  );
}

render(<App />);
