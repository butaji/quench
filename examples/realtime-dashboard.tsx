// Real-Time Dashboard — TuiBridge
// Demonstrates live updating data with animations
// Common pattern for monitoring tools, system stats

import { render, Box, Text, useState, useEffect, useApp, useInput } from 'ink';

interface Metric {
  name: string;
  value: number;
  unit: string;
  max: number;
  color: string;
}

function MetricBar({ metric }: { metric: Metric }): JSX.Element {
  const percentage = Math.min(100, (metric.value / metric.max) * 100);
  const filled = Math.round(percentage / 5); // 20 chars total
  const empty = 20 - filled;
  
  const bar = '█'.repeat(filled) + '░'.repeat(empty);
  const barColor = percentage > 80 ? 'red' : percentage > 60 ? 'yellow' : 'green';

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box flexDirection="row" justifyContent="space-between">
        <Text>{metric.name}</Text>
        <Text>{metric.value.toFixed(1)}{metric.unit}</Text>
      </Box>
      <Text color={barColor}>{bar}</Text>
    </Box>
  );
}

function generateMetrics(): Metric[] {
  return [
    { name: 'CPU', value: Math.random() * 100, unit: '%', max: 100, color: 'cyan' },
    { name: 'Memory', value: Math.random() * 100, unit: '%', max: 100, color: 'blue' },
    { name: 'Disk', value: Math.random() * 100, unit: '%', max: 100, color: 'yellow' },
    { name: 'Network', value: Math.random() * 1000, unit: 'MB/s', max: 1000, color: 'magenta' },
  ];
}

function RealTimeDashboard(): JSX.Element {
  const [metrics, setMetrics] = useState<Metric[]>(generateMetrics);
  const [paused, setPaused] = useState(false);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (paused) return;
    
    const interval = setInterval(() => {
      setMetrics(generateMetrics());
      setTick(t => t + 1);
    }, 500);

    return () => clearInterval(interval);
  }, [paused]);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === ' ') setPaused(p => !p);
    if (input === 'r') setMetrics(generateMetrics());
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Box flexDirection="row" justifyContent="space-between">
        <Text bold color="green">Real-Time Dashboard</Text>
        <Text dimColor>Tick: {tick}</Text>
      </Box>
      <Text dimColor>[Space] pause | [r] reset | [q] quit</Text>
      
      {paused && (
        <Text color="yellow" bold>[PAUSED]</Text>
      )}

      <Text> </Text>

      <Box flexDirection="row" gap={3}>
        {/* CPU & Memory */}
        <Box flexDirection="column" flexGrow={1}>
          <Text bold>System</Text>
          <Box borderStyle="single" padding={1}>
            <MetricBar metric={metrics[0]} />
            <MetricBar metric={metrics[1]} />
          </Box>
        </Box>

        {/* Disk & Network */}
        <Box flexDirection="column" flexGrow={1}>
          <Text bold>Storage</Text>
          <Box borderStyle="single" padding={1}>
            <MetricBar metric={metrics[2]} />
            <MetricBar metric={metrics[3]} />
          </Box>
        </Box>
      </Box>

      <Text> </Text>
      
      {/* Mini sparklines */}
      <Box flexDirection="row" gap={2}>
        {metrics.map((m, i) => (
          <Box key={m.name} flexDirection="column">
            <Text dimColor small>{m.name}</Text>
            <Text color={m.color}>{'▁▂▃▄▅▆▇█'[Math.floor(Math.random() * 8)]}</Text>
          </Box>
        ))}
      </Box>

      <Text> </Text>
      <Text dimColor small>Last update: {new Date().toLocaleTimeString()}</Text>
    </Box>
  );
}

render(<RealTimeDashboard />);
