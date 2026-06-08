// Entry point for ink-reference-directive example
// Run with: deno run -A main.tsx

import React from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

const { unmount } = render(<App />);
