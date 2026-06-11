// Context Demo — Quench
// Demonstrates useContext / createContext for theme sharing

import { render, Box, Text, useState, useContext, createContext, useInput, useApp } from 'ink';

interface Theme {
  name: string;
  fg: string;
  bg: string;
  accent: string;
}

const THEMES: Record<string, Theme> = {
  dark:  { name: 'dark',  fg: 'white', bg: '#1a1a1a', accent: 'cyan' },
  light: { name: 'light', fg: 'black', bg: 'white', accent: 'blue' },
  retro: { name: 'retro', fg: 'green', bg: 'black', accent: 'yellow' },
};

const ThemeContext = createContext<Theme>(THEMES.dark);

function ThemedBox({ children }: { children: JSX.Element | JSX.Element[] }) {
  const theme = useContext(ThemeContext);
  return (
    <Box borderStyle="round" borderColor={theme.accent} padding={1} backgroundColor={theme.bg}>
      {children}
    </Box>
  );
}

function ThemedText({ bold, children }: { bold?: boolean; children: string }) {
  const theme = useContext(ThemeContext);
  return <Text color={theme.fg} bold={bold}>{children}</Text>;
}

function ContextDemo(): JSX.Element {
  const [themeName, setThemeName] = useState<string>('dark');
  const theme = THEMES[themeName];

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === '1') setThemeName('dark');
    if (input === '2') setThemeName('light');
    if (input === '3') setThemeName('retro');
  });

  return (
    <ThemeContext.Provider value={theme}>
      <Box flexDirection="column" padding={1}>
        <ThemedText bold>Context Theme Demo</ThemedText>
        <ThemedText>[1-3] switch theme | [q] quit</ThemedText>
        <Text> </Text>
        <ThemedBox>
          <ThemedText>Current theme: {theme.name}</ThemedText>
          <ThemedText>Foreground: {theme.fg}</ThemedText>
          <ThemedText>Background: {theme.bg}</ThemedText>
          <ThemedText>Accent: {theme.accent}</ThemedText>
        </ThemedBox>
        <Text> </Text>
        <Box flexDirection="row" gap={1}>
          <Text color={themeName === 'dark' ? 'cyan' : 'gray'}>[1] Dark</Text>
          <Text color={themeName === 'light' ? 'blue' : 'gray'}>[2] Light</Text>
          <Text color={themeName === 'retro' ? 'yellow' : 'gray'}>[3] Retro</Text>
        </Box>
      </Box>
    </ThemeContext.Provider>
  );
}

render(<ContextDemo />);
