// Dashboard Example - TuiBridge demo (TypeScript)
// Demonstrates layout with multiple sections, live stats, and borders

import { render, Box, Text, useState, useEffect, useInput, useApp } from 'ink';

interface Metrics {
  cpu: number;
  memory: number;
  disk: number;
  network: number;
}

interface ProgressBarProps {
  label: string;
  value: number;
  max?: number;
  width?: number;
}

function ProgressBar(props: ProgressBarProps): JSX.Element {
  const { label, value, max = 100, width = 20 } = props;
  
  const percent = Math.round((value / max) * 100);
  const filled = Math.round((value / max) * width);
  const empty = width - filled;
  
  const bar = '█'.repeat(filled) + '░'.repeat(empty);
  const color = percent > 80 ? 'green' : percent > 50 ? 'yellow' : 'red';
  
  return (
    <Box flexDirection="column" margin={1}>
      <Text>{label}: {percent}%</Text>
      <Text color={color}>[{bar}]</Text>
    </Box>
  );
}

function App(): JSX.Element {
  const [metrics, setMetrics] = useState<Metrics>({
    cpu: 45,
    memory: 67,
    disk: 23,
    network: 89,
  });
  const [uptime, setUptime] = useState(0);
  
  useEffect(() => {
    const timer = setInterval(() => {
      setMetrics((m: Metrics) => ({
        cpu: Math.max(0, Math.min(100, m.cpu + Math.round((Math.random() - 0.5) * 20))),
        memory: Math.max(0, Math.min(100, m.memory + Math.round((Math.random() - 0.5) * 5))),
        disk: 23,
        network: Math.max(0, Math.min(100, m.network + Math.round((Math.random() - 0.5) * 30))),
      }));
      setUptime((u: number) => u + 1);
    }, 1000);
    
    return () => clearInterval(timer);
  }, []);
  
  useInput((input: string) => {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
  });
  
  const hours = Math.floor(uptime / 3600);
  const minutes = Math.floor((uptime % 3600) / 60);
  const seconds = uptime % 60;
  
  return (
    <Box flexDirection="column" padding={1}>
      <Box borderStyle="bold">
        <Text color="green" bold>System Dashboard</Text>
        <Text dimColor>   Uptime: {hours}h {minutes}m {seconds}s</Text>
      </Box>
      <Text> </Text>
      <Box>
        <Box borderStyle="round" padding={1} margin={1}>
          <Text dimColor>CPU</Text>
          <Text bold color={metrics.cpu > 80 ? 'red' : 'green'}>{metrics.cpu}%</Text>
        </Box>
        <Box borderStyle="round" padding={1} margin={1}>
          <Text dimColor>Memory</Text>
          <Text bold color={metrics.memory > 80 ? 'red' : 'yellow'}>{metrics.memory}%</Text>
        </Box>
        <Box borderStyle="round" padding={1} margin={1}>
          <Text dimColor>Disk</Text>
          <Text bold color="cyan">{metrics.disk}%</Text>
        </Box>
        <Box borderStyle="round" padding={1} margin={1}>
          <Text dimColor>Network</Text>
          <Text bold color="magenta">{metrics.network} Mbps</Text>
        </Box>
      </Box>
      <Text> </Text>
      <Box flexDirection="column" borderStyle="round">
        <Text bold>Resource Usage</Text>
        <ProgressBar label="CPU" value={metrics.cpu} />
        <ProgressBar label="Memory" value={metrics.memory} />
        <ProgressBar label="Disk" value={metrics.disk} />
        <ProgressBar label="Network" value={metrics.network} />
      </Box>
      <Text dimColor>[q] quit</Text>
    </Box>
  );
}

render(<App />);
