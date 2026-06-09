// ink-class-expression example — demonstrates anonymous class expressions.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Simple anonymous class expression
const Counter = class {
  count = 0;

  increment(): number {
    return ++this.count;
  }

  decrement(): number {
    return --this.count;
  }

  getCount(): number {
    return this.count;
  }
};

// Anonymous class with constructor
const Person = class {
  name: string;
  age: number;

  constructor(name: string, age: number) {
    this.name = name;
    this.age = age;
  }

  greet(): string {
    return `Hello, I'm ${this.name} and I'm ${this.age}`;
  }
};

// Anonymous class extending another class
const BaseModel = class {
  id: number;

  constructor(id: number) {
    this.id = id;
  }

  getId(): number {
    return this.id;
  }
};

const ExtendedModel = class extends BaseModel {
  name: string;

  constructor(id: number, name: string) {
    super(id);
    this.name = name;
  }

  describe(): string {
    return `Model #${this.getId()}: ${this.name}`;
  }
};

// Immediately invoked anonymous class
const singleton = new (class {
  value = 42;

  getValue(): number {
    return this.value;
  }
})();

// Anonymous class with static member
const Calculator = class {
  static version = '1.0';

  static add(a: number, b: number): number {
    return a + b;
  }

  value: number;

  constructor(value: number = 0) {
    this.value = value;
  }

  multiply(factor: number): number {
    this.value *= factor;
    return this.value;
  }
};

export default function ClassExpressionDemo() {
  const results: string[] = [];

  // Basic anonymous class usage
  const counter = new Counter();
  results.push(`Counter initial: ${counter.getCount()}`);
  counter.increment();
  counter.increment();
  results.push(`After 2 increments: ${counter.getCount()}`);
  counter.decrement();
  results.push(`After decrement: ${counter.getCount()}`);

  // Anonymous class with constructor
  const person = new Person('Alice', 30);
  results.push(`Person: ${person.greet()}`);

  const person2 = new Person('Bob', 25);
  results.push(`Person2: ${person2.greet()}`);

  // Anonymous class extending another anonymous class
  const model = new ExtendedModel(1, 'Widget');
  results.push(`Model: ${model.describe()}`);

  // Immediately invoked
  results.push(`Singleton value: ${singleton.getValue()}`);

  // Static member anonymous class
  results.push(`Calc version: ${Calculator.version}`);
  results.push(`Calc 3+4: ${Calculator.add(3, 4)}`);
  const calc = new Calculator(5);
  results.push(`Calc value: ${calc.multiply(3)}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Class Expression Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
