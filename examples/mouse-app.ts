// Mouse App Example - TuiBridge demo (TypeScript)
// Demonstrates click tracking and mouse events

import { render, Box, Text, useState, useInput, useEffect } from 'ink';

interface ClickEvent {
  x: number;
  y: number;
  time: string;
}

function MouseApp(): JSX.Element {
  const [clicks, setClicks] = useState<ClickEvent[]>([]);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const [hoverTarget, setHoverTarget] = useState<string | null>(null);
  
  // Note: useInput doesn't directly expose mouse position in current implementation
  // This is a simplified demo showing the concept
  
  useEffect(() => {
    // Simulate mouse position updates (in real app, this comes from terminal events)
    const timer = setInterval(() => {
      setMousePos((p: {x: number, y: number}) => ({
        x: Math.floor(Math.random() * 60) + 10,
        y: Math.floor(Math.random() * 10) + 5,
      }));
    }, 500);
    
    return () => clearInterval(timer);
  }, []);
  
  useInput((input: string) => {
    // Simulate click with keyboard (since actual mouse requires terminal support)
    if (input === ' ' || input === 'enter') {
      const now = new Date();
      const time = now.toTimeString().slice(0, 8);
      
      setClicks((c: ClickEvent[]) => [
        ...c,
        { x: mousePos.x, y: mousePos.y, time },
      ].slice(-5)); // Keep last 5 clicks
      
      // Determine hover target based on position
      if (mousePos.y >= 3 && mousePos.y <= 5) {
        setHoverTarget('Button A');
      } else if (mousePos.y >= 7 && mousePos.y <= 9) {
        setHoverTarget('Button B');
      } else if (mousePos.y >= 11 && mousePos.y <= 13) {
        setHoverTarget('Button C');
      } else {
        setHoverTarget(null);
      }
    }
    if (input === 'q' || input === 'Q') {
      process.exit(0);
    }
  });
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="green">Mouse Demo</Text>
      <Text dimColor>Position: ({mousePos.x}, {mousePos.y}) | Hover: {hoverTarget || 'none'}</Text>
      
      <Box marginTop={1} borderStyle="round" padding={1} flexDirection="column">
        <Box justifyContent="center" marginY={1}>
          <Box 
            width={15} 
            height={3} 
            borderStyle="single"
            backgroundColor={hoverTarget === 'Button A' ? 'gray' : undefined}
            justifyContent="center"
            alignItems="center"
          >
            <Text>Button A</Text>
          </Box>
        </Box>
        <Box justifyContent="center" marginY={1}>
          <Box 
            width={15} 
            height={3} 
            borderStyle="single"
            backgroundColor={hoverTarget === 'Button B' ? 'gray' : undefined}
            justifyContent="center"
            alignItems="center"
          >
            <Text>Button B</Text>
          </Box>
        </Box>
        <Box justifyContent="center" marginY={1}>
          <Box 
            width={15} 
            height={3} 
            borderStyle="single"
            backgroundColor={hoverTarget === 'Button C' ? 'gray' : undefined}
            justifyContent="center"
            alignItems="center"
          >
            <Text>Button C</Text>
          </Box>
        </Box>
      </Box>
      
      <Box marginTop={1}>
        <Text bold>Recent Clicks:</Text>
        {clicks.length === 0 && <Text dimColor> (none yet)</Text>}
        {clicks.map((click: ClickEvent, i: number) => (
          <Text key={i} dimColor> [{click.time}] ({click.x},{click.y})</Text>
        ))}
      </Box>
      
      <Text dimColor marginTop={1}>
        [space/enter] simulate click | [q] quit
      </Text>
    </Box>
  );
}

render(<MouseApp />);
