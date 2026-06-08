// Main entry point - renders the tui/app.tsx component
import { render } from 'ink';
import React from 'react';
import App from './tui/app.tsx';

render(React.createElement(App));
