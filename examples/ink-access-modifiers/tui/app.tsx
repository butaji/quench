// Access Modifiers example — public, private, protected, readonly
//
// TypeScript access modifiers are used for encapsulation.
// They are erased or mapped to Rust visibility at compile time.

import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  // private fields are only accessible within the class
  private count: number = 0;
  
  // public fields are accessible everywhere (default)
  public readonly name: string;
  
  // protected fields are accessible within class and subclasses
  protected max: number;

  constructor(name: string, max: number) {
    this.name = name;
    this.max = max;
  }

  // public methods are accessible everywhere
  public increment(): void {
    if (this.count < this.max) {
      this.count++;
    }
  }

  public getValue(): number {
    return this.count;
  }
}

class ReadOnlyCounter extends Counter {
  public getCount(): number {
    // Can access protected max here
    return this.getValue();
  }
}

const counter = new Counter('Items', 5);
counter.increment();
counter.increment();

const readonlyCounter = new ReadOnlyCounter('ReadOnly', 3);
readonlyCounter.increment();

export default function App() {
  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>Access Modifiers Demo</Text>
      <Text>{counter.name}: {counter.getValue()}</Text>
      <Text>{readonlyCounter.name}: {readonlyCounter.getCount()}</Text>
      <Text dimColor>(private/protected/readonly erased)</Text>
    </Box>
  );
}
