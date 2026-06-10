// Progress Bar — TuiBridge
// Visual progress indicator with percentage
// Common pattern for long-running operations

import { render, Box, Text, useState, useEffect, useInput, useApp } from 'ink';

function ProgressBar({ 
  value, 
  max = 100, 
  width = 40,
  color = 'cyan',
}: { 
  value: number; 
  max?: number; 
  width?: number;
  color?: string;
}): JSX.Element {
  const percent = Math.min(100, Math.max(0, (value / max) * 100));
  const filled = Math.round((percent / 100) * width);
  const empty = width - filled;
  
  const bar = '█'.repeat(filled) + '░'.repeat(empty);
  const percentStr = `${Math.round(percent)}%`.padStart(4);

  return (
    <Box flexDirection="row" gap={1}>
      <Text color={color}>[{bar}]</Text>
      <Text>{percentStr}</Text>
    </Box>
  );
}

interface Task {
  id: string;
  name: string;
  progress: number;
  status: 'pending' | 'running' | 'done' | 'error';
}

function MultiProgressDemo(): JSX.Element {
  const [tasks, setTasks] = useState<Task[]>([
    { id: '1', name: 'Downloading', progress: 0, status: 'pending' },
    { id: '2', name: 'Extracting', progress: 0, status: 'pending' },
    { id: '3', name: 'Compiling', progress: 0, status: 'pending' },
    { id: '4', name: 'Installing', progress: 0, status: 'pending' },
  ]);

  useEffect(() => {
    // Simulate progress for each task
    const intervals: ReturnType<typeof setInterval>[] = [];
    
    tasks.forEach((task, index) => {
      if (task.status !== 'pending') return;
      
      const delay = index * 1500; // Stagger start times
      
      const startTimeout = setTimeout(() => {
        setTasks(prev => prev.map((t, i) => 
          i === index ? { ...t, status: 'running' } : t
        ));
        
        const interval = setInterval(() => {
          setTasks(prev => prev.map((t, i) => {
            if (i !== index || t.status !== 'running') return t;
            
            const newProgress = t.progress + Math.random() * 15;
            if (newProgress >= 100) {
              clearInterval(interval);
              return { ...t, progress: 100, status: 'done' };
            }
            return { ...t, progress: newProgress };
          }));
        }, 200);
        
        intervals.push(interval);
      }, delay);
      
      intervals.push(startTimeout as unknown as ReturnType<typeof setInterval>);
    });

    return () => {
      intervals.forEach(clearInterval);
      intervals.forEach(clearTimeout);
    };
  }, []);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
  });

  const allDone = tasks.every(t => t.status === 'done');

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Progress Bar Demo</Text>
      <Text dimColor>[q] quit</Text>
      <Text> </Text>

      {tasks.map(task => (
        <Box key={task.id} flexDirection="column" marginBottom={1}>
          <Box flexDirection="row" justifyContent="space-between" width={50}>
            <Text>
              {task.status === 'done' && <Text color="green">✓ </Text>}
              {task.status === 'running' && <Text color="cyan">► </Text>}
              {task.name}
            </Text>
            <Text dimColor>
              {task.status === 'done' ? 'Done' : 
               task.status === 'running' ? 'Running...' : 'Waiting'}
            </Text>
          </Box>
          <ProgressBar 
            value={task.progress} 
            color={task.status === 'done' ? 'green' : 'cyan'}
          />
        </Box>
      ))}

      {allDone && (
        <Box borderStyle="single" borderColor="green" padding={1} marginTop={1}>
          <Text color="green" bold>✓ All tasks completed!</Text>
        </Box>
      )}
    </Box>
  );
}

render(<MultiProgressDemo />);
