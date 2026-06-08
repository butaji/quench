// ink-private-methods — demonstrates ES2022 private methods and `in` operator for private fields.
//
// Private methods (#method()) and the `in` operator for private fields (#field in obj)
// are ES2022 features for true encapsulation.
//
// Dev path (rquickjs): Both features work correctly with 100% parity to deno.
// Compile path: Private methods compile correctly. The `in` operator for private
// fields generates compilable Rust but cannot correctly check private field presence
// at codegen time (known architectural limitation).
//
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  #count: number = 0;

  // Private method - #validate is only callable from within the class
  #validate(n: number): boolean {
    return n >= 0;
  }

  // Public method that uses the private method
  increment(): void {
    if (this.#validate(this.#count + 1)) {
      this.#count++;
    }
  }

  decrement(): void {
    if (this.#validate(this.#count - 1)) {
      this.#count--;
    }
  }

  // Public method to get the current count
  getValue(): number {
    return this.#count;
  }

  // Static method using private fields
  static hasCount(obj: unknown): boolean {
    // The `in` operator for private fields is ES2022
    // This checks if obj has the #count private field
    if (obj instanceof Counter) {
      return #count in obj;
    }
    return false;
  }
}

// Another class with private methods
class Stack {
  #items: string[] = [];
  #maxSize: number;

  constructor(maxSize: number = 10) {
    this.#maxSize = maxSize;
  }

  #isFull(): boolean {
    return this.#items.length >= this.#maxSize;
  }

  #isEmpty(): boolean {
    return this.#items.length === 0;
  }

  push(item: string): boolean {
    if (this.#isFull()) {
      return false;
    }
    this.#items.push(item);
    return true;
  }

  pop(): string | undefined {
    if (this.#isEmpty()) {
      return undefined;
    }
    return this.#items.pop();
  }

  getSize(): number {
    return this.#items.length;
  }
}

const counter = new Counter();
counter.increment();
counter.increment();
counter.increment();

const stack = new Stack(3);
stack.push('a');
stack.push('b');
stack.push('c');
const overflow = stack.push('d'); // Should fail - stack is full

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>=== Private Methods Demo ===</Text>
      <Text>Counter value: {counter.getValue()}</Text>
      <Text>Counter has #count: {Counter.hasCount(counter) ? 'yes' : 'no'}</Text>
      <Text>Other has #count: {Counter.hasCount({}) ? 'yes' : 'no'}</Text>
      <Text>---</Text>
      <Text>Stack size: {stack.getSize()}</Text>
      <Text>Push overflow: {overflow ? 'true' : 'false'}</Text>
    </Box>
  );
}
