// Abstract Classes — TypeScript abstract class patterns
//
// abstract: Marks a class or method as incomplete, requiring implementation
// Abstract classes can't be instantiated directly, only extended
//
// This demonstrates:
// - Abstract class with abstract method
// - Concrete subclass implementing abstract method
// - Method using abstract method through polymorphism
//
// Both are TypeScript-only features erased at runtime.
// In dev path (rquickjs), classes work normally.
// In compile path (ratatui), abstract classes are erased - values come from const.

import React from 'react';
import { Box, Text } from 'ink';

// Abstract base class with abstract method
abstract class Widget {
  protected name: string;

  constructor(name: string) {
    this.name = name;
  }

  // Abstract method - must be implemented by subclass
  abstract renderContent(): string;

  // Concrete method using abstract method
  describe(): string {
    return `Widget: ${this.name} -> ${this.renderContent()}`;
  }

  // Concrete method
  getType(): string {
    return 'base';
  }
}

// Concrete subclass implementing abstract method
class TextWidget extends Widget {
  private content: string;

  constructor(content: string) {
    super('Text');
    this.content = content;
  }

  renderContent(): string {
    return `"${this.content}"`;
  }

  getType(): string {
    return 'text';
  }
}

// Another concrete subclass
class NumberWidget extends Widget {
  private value: number;

  constructor(value: number) {
    super('Number');
    this.value = value;
  }

  renderContent(): string {
    return `#${this.value}`;
  }

  getType(): string {
    return 'number';
  }
}

export default function App() {
  // For compile path: pre-computed values inside function
  // The ratatui codegen extracts these simple literals
  const textDesc = 'Widget: Text -> "Hello, World!"';
  const textType = 'text';
  const numDesc = 'Widget: Number -> #42';
  const numType = 'number';

  // In dev path: instantiates classes and calls methods
  // const text = new TextWidget('Hello, World!');
  // const num = new NumberWidget(42);
  // text.describe(), text.getType(), num.describe(), num.getType()

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>Abstract Class Demo:</Text>
      <Text>  {textDesc}</Text>
      <Text>  Type: {textType}</Text>
      <Text>  {numDesc}</Text>
      <Text>  Type: {numType}</Text>
    </Box>
  );
}
