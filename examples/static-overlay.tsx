// Static Overlay Demo — TuiBridge
// Demonstrates Static, Newline, Spacer components

import { render, Box, Text, Static, Newline, Spacer, useState, useEffect, useInput, useApp } from 'ink';

function StaticOverlayDemo(): JSX.Element {
  const [items, setItems] = useState<string[]>([]);

  useEffect(() => {
    const words = ['Loading', 'Parsing', 'Compiling', 'Optimizing', 'Done'];
    let i = 0;
    const timer = setInterval(() => {
      if (i < words.length) {
        setItems(prev => [...prev, words[i]]);
        i++;
      } else {
        clearInterval(timer);
      }
    }, 800);
    return () => clearInterval(timer);
  }, []);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="green">Static, Newline, Spacer Demo</Text>
      <Text dimColor>[q] quit</Text>
      <Newline />
      <Text>Static items (persist above):</Text>
      <Static items={items}>
        {(item: string) => <Text key={item} color="cyan">✓ {item}</Text>}
      </Static>
      <Newline />
      <Text>Dynamic area below:</Text>
      <Box flexDirection="row">
        <Text>Left</Text>
        <Spacer />
        <Text>Right (spacer pushes this)</Text>
      </Box>
      <Newline />
      <Text dimColor>Newline forces blank lines. Spacer fills flex space.</Text>
    </Box>
  );
}

render(<StaticOverlayDemo />);
