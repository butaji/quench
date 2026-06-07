// Barrel Export Pattern — export * from, import * as namespace
//
// Barrel files re-export from multiple modules.
// import * as creates a namespace object with all exports.

import React from 'react';
import { Box, Text } from 'ink';

// Simulated barrel export (would be `export * from './modules.js'`)
// In a real project, these would be in separate files
const modules = {
  header: { title: 'My App' },
  body: { content: 'Welcome to the demo' },
  footer: { version: '1.0.0' }
};

// Simulated namespace import
// import * as Components from './components/index.js';
const Components = modules;

function Header() {
  return <Text bold>{Components.header.title}</Text>;
}

function Body() {
  return <Text>{Components.body.content}</Text>;
}

function Footer() {
  return <Text dimColor>Version {Components.footer.version}</Text>;
}

// Barrel export simulation
// export { Header, Body, Footer };
// export default function App() { ... }

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Header />
      <Body />
      <Footer />
    </Box>
  );
}
