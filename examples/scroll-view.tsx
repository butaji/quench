// Scroll View Demo — TuiBridge
// Demonstrates scrolling content with keyboard navigation
// Common pattern for long lists, logs, file viewers

import { render, Box, Text, useState, useEffect, useInput, useApp } from 'ink';

interface LogEntry {
  timestamp: string;
  level: 'info' | 'warn' | 'error';
  message: string;
}

const LOGS: LogEntry[] = [
  { timestamp: '10:23:01', level: 'info', message: 'Application started' },
  { timestamp: '10:23:02', level: 'info', message: 'Loading configuration...' },
  { timestamp: '10:23:03', level: 'info', message: 'Connecting to database' },
  { timestamp: '10:23:04', level: 'warn', message: 'Slow query detected (>100ms)' },
  { timestamp: '10:23:05', level: 'info', message: 'User authenticated: admin' },
  { timestamp: '10:23:06', level: 'info', message: 'Request processed: /api/users' },
  { timestamp: '10:23:07', level: 'error', message: 'Connection timeout: redis://cache' },
  { timestamp: '10:23:08', level: 'info', message: 'Retrying connection...' },
  { timestamp: '10:23:09', level: 'info', message: 'Cache reconnected successfully' },
  { timestamp: '10:23:10', level: 'info', message: 'Background job started: cleanup' },
  { timestamp: '10:23:11', level: 'warn', message: 'Memory usage: 78%' },
  { timestamp: '10:23:12', level: 'info', message: 'Background job completed' },
  { timestamp: '10:23:13', level: 'info', message: 'Heartbeat sent' },
  { timestamp: '10:23:14', level: 'error', message: 'API rate limit exceeded' },
  { timestamp: '10:23:15', level: 'info', message: 'Rate limit reset' },
];

function ScrollView({ items, visibleHeight }: { 
  items: string[]; 
  visibleHeight: number;
}): JSX.Element {
  const [scrollTop, setScrollTop] = useState(0);
  
  useInput((input: string, key: { downArrow: boolean; upArrow: boolean }) => {
    if (key.downArrow || input === 'j') {
      setScrollTop(prev => Math.min(prev + 1, Math.max(0, items.length - visibleHeight)));
    }
    if (key.upArrow || input === 'k') {
      setScrollTop(prev => Math.max(prev - 1, 0));
    }
    if (input === 'g') {
      setScrollTop(0);
    }
    if (input === 'G') {
      setScrollTop(Math.max(0, items.length - visibleHeight));
    }
  });

  const visibleItems = items.slice(scrollTop, scrollTop + visibleHeight);
  const maxScroll = Math.max(0, items.length - visibleHeight);

  return (
    <Box flexDirection="column">
      {visibleItems.map((item, index) => (
        <Box key={scrollTop + index}>
          <Text dimColor>{String(scrollTop + index + 1).padStart(3)} │ </Text>
          <Text>{item}</Text>
        </Box>
      ))}
      {/* Scroll indicator */}
      <Box flexDirection="row" marginTop={1}>
        <Text dimColor>
          Lines {scrollTop + 1}-{scrollTop + visibleItems.length} of {items.length}
          {maxScroll > 0 && ` (scroll: ${scrollTop}/${maxScroll})`}
        </Text>
      </Box>
    </Box>
  );
}

function LogViewer(): JSX.Element {
  const [logs] = useState(LOGS);
  const VISIBLE_LINES = 8;

  const formattedLogs = logs.map(log => {
    const levelColor = log.level === 'error' ? 'red' : 
                       log.level === 'warn' ? 'yellow' : 'gray';
    return `[${log.timestamp}] [${log.level.toUpperCase().padEnd(5)}] ${log.message}`;
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Log Viewer (Scroll Demo)</Text>
      <Text dimColor>[↑/↓ or k/j] scroll | [g/G] top/bottom | [q] quit</Text>
      <Text> </Text>
      <Box borderStyle="single" padding={1}>
        <ScrollView items={formattedLogs} visibleHeight={VISIBLE_LINES} />
      </Box>
    </Box>
  );
}

function ScrollViewDemo(): JSX.Element {
  const [view, setView] = useState<'logs' | 'files'>('logs');
  const files = [
    'src/main.rs',
    'src/bridge.rs',
    'src/ink.rs',
    'src/runtime.js',
    'src/hotreload.rs',
    'examples/counter.tsx',
    'examples/todo-list.tsx',
    'examples/dashboard.tsx',
    'examples/context-demo.tsx',
    'examples/focus-manager.tsx',
    'examples/wizard.tsx',
    'docs/SPEC.md',
    'Cargo.toml',
    'package.json',
  ];

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'l') setView('logs');
    if (input === 'f') setView('files');
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Scroll View Demo</Text>
      <Text dimColor>[l] logs | [f] files | [q] quit</Text>
      <Text> </Text>
      
      <Box borderStyle="single" padding={1} height={12}>
        {view === 'logs' ? (
          <ScrollView items={formattedLogs || logs.map(l => l.message)} visibleHeight={10} />
        ) : (
          <ScrollView items={files} visibleHeight={10} />
        )}
      </Box>
      
      <Text dimColor small>
        Try: ↑↓ to scroll, g for top, G for bottom
      </Text>
    </Box>
  );
}

// Helper for formatted logs
const formattedLogs = LOGS.map(log => {
  const levelColor = log.level === 'error' ? 'red' : 
                     log.level === 'warn' ? 'yellow' : 'gray';
  return `[${log.timestamp}] [${log.level.toUpperCase().padEnd(5)}] ${log.message}`;
});

render(<ScrollViewDemo />);
