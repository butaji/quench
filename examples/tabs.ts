// Tabs Example - TuiBridge demo (TypeScript)
// Demonstrates tab navigation and dynamic content

import { render, Box, Text, useState, useInput } from 'ink';

interface Tab {
  name: string;
  content: string[];
}

function TabsApp(): JSX.Element {
  const tabs: Tab[] = [
    {
      name: 'Overview',
      content: [
        'Welcome to TuiBridge!',
        '',
        'This is a terminal-based UI framework',
        'built with Rust and JavaScript.',
        '',
        'Features:',
        '• High-performance rendering',
        '• React-like component model',
        '• Yoga layout engine',
      ],
    },
    {
      name: 'Settings',
      content: [
        'Settings',
        '========',
        '',
        'Theme: Dark',
        'Font Size: 14px',
        'Terminal: xterm-256color',
        '',
        '[Not editable in demo]',
      ],
    },
    {
      name: 'Help',
      content: [
        'Keyboard Shortcuts',
        '==================',
        '',
        'j/k - Navigate',
        'tab - Next tab',
        'q   - Quit',
        '',
        'For more help, see the docs.',
      ],
    },
    {
      name: 'About',
      content: [
        'TuiBridge',
        '=========',
        '',
        'Version: 0.1.0',
        '',
        'A bridge between React/Ink',
        'and the terminal.',
        '',
        '© 2024 TuiBridge Team',
      ],
    },
  ];
  
  const [activeTab, setActiveTab] = useState(0);
  
  useInput((input: string) => {
    if (input === 'tab') {
      setActiveTab((t: number) => (t + 1) % tabs.length);
    }
    if (input === 'S-tab') {
      setActiveTab((t: number) => (t - 1 + tabs.length) % tabs.length);
    }
    if (input === 'left') {
      setActiveTab((t: number) => (t - 1 + tabs.length) % tabs.length);
    }
    if (input === 'right') {
      setActiveTab((t: number) => (t + 1) % tabs.length);
    }
    if (input === 'q' || input === 'Q') {
      process.exit(0);
    }
  });
  
  return (
    <Box flexDirection="column" padding={1}>
      <Box>
        {tabs.map((tab: Tab, i: number) => (
          <Box
            key={i}
            paddingX={1}
            backgroundColor={i === activeTab ? 'gray' : undefined}
            borderStyle={i === activeTab ? 'round' : undefined}
          >
            <Text bold={i === activeTab}>
              {i === activeTab ? '> ' : '  '}{tab.name}
            </Text>
          </Box>
        ))}
      </Box>
      <Box marginTop={1} borderStyle="round" padding={1} flexDirection="column">
        {tabs[activeTab].content.map((line: string, i: number) => (
          <Text key={i}>{line}</Text>
        ))}
      </Box>
      <Text dimColor marginTop={1}>
        [←/→] or [tab] switch tabs | [q] quit
      </Text>
    </Box>
  );
}

render(<TabsApp />);
