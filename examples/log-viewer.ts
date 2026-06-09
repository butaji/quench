// Log Viewer Example - TuiBridge demo (TypeScript)
// Demonstrates scrolling, filtering, and auto-scroll

import { render, Box, Text, useState, useInput, useEffect } from 'ink';

type LogLevel = 'INFO' | 'WARN' | 'ERROR' | 'DEBUG';

interface LogEntry {
  timestamp: string;
  level: LogLevel;
  message: string;
}

function LogViewer(): JSX.Element {
  const [logs, setLogs] = useState<LogEntry[]>([
    { timestamp: '10:00:00', level: 'INFO', message: 'Application started' },
    { timestamp: '10:00:01', level: 'DEBUG', message: 'Loading configuration...' },
    { timestamp: '10:00:02', level: 'INFO', message: 'Config loaded successfully' },
    { timestamp: '10:00:03', level: 'WARN', message: 'Cache miss for key: user_prefs' },
    { timestamp: '10:00:04', level: 'INFO', message: 'Starting background worker' },
    { timestamp: '10:00:05', level: 'ERROR', message: 'Failed to connect to database' },
    { timestamp: '10:00:06', level: 'INFO', message: 'Retrying connection...' },
    { timestamp: '10:00:07', level: 'INFO', message: 'Connection established' },
  ]);
  const [filter, setFilter] = useState<LogLevel | 'ALL'>('ALL');
  const [autoScroll, setAutoScroll] = useState(true);
  const [scrollOffset, setScrollOffset] = useState(0);
  
  useEffect(() => {
    const timer = setInterval(() => {
      const levels: LogLevel[] = ['INFO', 'WARN', 'ERROR', 'DEBUG'];
      const messages = [
        'Processing request...',
        'Query executed in 23ms',
        'Cache updated',
        'Worker thread spawned',
        'Connection pool: 5/10 active',
      ];
      
      const now = new Date();
      const timestamp = now.toTimeString().slice(0, 8);
      
      setLogs((l: LogEntry[]) => [
        ...l,
        {
          timestamp,
          level: levels[Math.floor(Math.random() * levels.length)],
          message: messages[Math.floor(Math.random() * messages.length)],
        },
      ].slice(-100)); // Keep last 100 entries
    }, 2000);
    
    return () => clearInterval(timer);
  }, []);
  
  useInput((input: string) => {
    if (input === 'j' || input === 'down') {
      setScrollOffset((o: number) => o + 1);
      setAutoScroll(false);
    }
    if (input === 'k' || input === 'up') {
      setScrollOffset((o: number) => Math.max(0, o - 1));
    }
    if (input === 'G') {
      setScrollOffset(9999);
      setAutoScroll(false);
    }
    if (input === 'g') {
      setScrollOffset(0);
      setAutoScroll(false);
    }
    if (input === 'a') {
      setAutoScroll((s: boolean) => !s);
    }
    if (input === '1') setFilter('ALL');
    if (input === '2') setFilter('INFO');
    if (input === '3') setFilter('WARN');
    if (input === '4') setFilter('ERROR');
    if (input === 'q' || input === 'Q') {
      process.exit(0);
    }
  });
  
  // Auto-scroll to bottom when enabled
  const displayLogs = filter === 'ALL' 
    ? logs 
    : logs.filter((l: LogEntry) => l.level === filter);
  
  const visibleLogs = autoScroll 
    ? displayLogs.slice(-10) 
    : displayLogs.slice(scrollOffset, scrollOffset + 10);
  
  const getLevelColor = (level: LogLevel): string => {
    switch (level) {
      case 'ERROR': return 'red';
      case 'WARN': return 'yellow';
      case 'INFO': return 'cyan';
      case 'DEBUG': return 'gray';
      default: return 'white';
    }
  };
  
  return (
    <Box flexDirection="column" padding={1}>
      <Box>
        <Text bold color="green">Log Viewer</Text>
        <Text dimColor> | Filter: {filter} | Auto-scroll: {autoScroll ? 'ON' : 'OFF'}</Text>
      </Box>
      <Box flexDirection="column" marginTop={1} borderStyle="round">
        {visibleLogs.map((log: LogEntry, i: number) => (
          <Box key={i}>
            <Text dimColor>{log.timestamp} </Text>
            <Text color={getLevelColor(log.level)}>[{log.level}] </Text>
            <Text>{log.message}</Text>
          </Box>
        ))}
      </Box>
      <Text dimColor marginTop={1}>
        [j/k] scroll | [g/G] top/bottom | [a] auto-scroll | [1-4] filter | [q] quit
      </Text>
    </Box>
  );
}

render(<LogViewer />);
