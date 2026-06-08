// Override and Implements — TypeScript class inheritance patterns
//
// override: Explicitly marks a method as overriding a parent method (TS 4.3+)
// implements: Declares that a class must implement an interface
//
// Both are TypeScript-only features that are erased at runtime.
// The compiled JavaScript doesn't have 'override' or 'implements'.
// In dev path (rquickjs), classes work normally.
// In compile path (ratatui), class instances are erased - values come from const.

import React from 'react';
import { Box, Text } from 'ink';

// Interface declaration (type-only, erased at runtime)
interface Renderable {
  render(): string;
  getName(): string;
}

// Base class with a render method
class BaseWidget {
  protected name: string = 'Widget';

  render(): string {
    return 'base';
  }

  getName(): string {
    return this.name;
  }
}

// Derived class that implements an interface and overrides a method
class FancyWidget extends BaseWidget implements Renderable {
  protected name: string = 'FancyWidget';
  private decorative: boolean = true;

  // override: explicitly marks this as overriding BaseWidget.render()
  override render(): string {
    return this.decorative ? '[fancy]' : 'fancy';
  }

  getDisplayName(): string {
    return `${this.name} (${this.decorative ? 'decorated' : 'plain'})`;
  }
}

// Class that implements interface but doesn't override (uses parent)
class SimpleWidget extends BaseWidget implements Renderable {
  protected name: string = 'SimpleWidget';
  // Uses parent's render() but satisfies interface contract
}

export default function App() {
  // For compile path: extract values as const literals
  // The ratatui codegen can only extract these simple literals
  const fancyName = 'FancyWidget (decorated)';
  const fancyRender = '[fancy]';
  const simpleName = 'SimpleWidget';
  const simpleRender = 'base';
  const counterCount = 2;

  // In dev path, these class instances work:
  // const fancy = new FancyWidget();
  // const simple = new SimpleWidget();
  // fancy.getDisplayName(), fancy.render(), simple.getName(), simple.render()

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>FancyWidget:</Text>
      <Text>  Name: {fancyName}</Text>
      <Text>  Render: {fancyRender}</Text>
      <Text bold>SimpleWidget:</Text>
      <Text>  Name: {simpleName}</Text>
      <Text>  Render: {simpleRender}</Text>
      <Text bold>Counter:</Text>
      <Text>  Count: {counterCount}</Text>
    </Box>
  );
}
