// ink-class-component example — demonstrates classes, extends, and super.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Simple class with constructor
class Counter {
  private count: number;

  constructor(initial: number = 0) {
    this.count = initial;
  }

  increment(): number {
    this.count += 1;
    return this.count;
  }

  getCount(): number {
    return this.count;
  }

  reset(): void {
    this.count = 0;
  }
}

// Class with inheritance
class Animal {
  protected name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return `${this.name} makes a sound`;
  }

  getName(): string {
    return this.name;
  }
}

class Dog extends Animal {
  private breed: string;

  constructor(name: string, breed: string) {
    super(name);
    this.breed = breed;
  }

  speak(): string {
    return `${this.name} barks: Woof!`;
  }

  getBreed(): string {
    return this.breed;
  }
}

class Cat extends Animal {
  private indoor: boolean;

  constructor(name: string, indoor: boolean = true) {
    super(name);
    this.indoor = indoor;
  }

  speak(): string {
    return `${this.name} meows: Meow!`;
  }

  isIndoor(): string {
    return this.indoor ? 'indoor' : 'outdoor';
  }
}

// Static method class
class MathHelper {
  static add(a: number, b: number): number {
    return a + b;
  }

  static multiply(a: number, b: number): number {
    return a * b;
  }

  static readonly PI = 3.14159;

  static circleArea(radius: number): number {
    return MathHelper.PI * radius * radius;
  }
}

export default function ClassDemo() {
  const results: string[] = [];

  // Basic class usage
  const counter = new Counter(10);
  results.push(`Initial count: ${counter.getCount()}`);
  counter.increment();
  counter.increment();
  results.push(`After 2 increments: ${counter.getCount()}`);
  counter.reset();
  results.push(`After reset: ${counter.getCount()}`);

  // Inheritance
  const dog = new Dog('Buddy', 'Golden Retriever');
  results.push(`Dog: ${dog.getName()}`);
  results.push(`Breed: ${dog.getBreed()}`);
  results.push(`Says: ${dog.speak()}`);

  const cat = new Cat('Whiskers', false);
  results.push(`Cat: ${cat.getName()}`);
  results.push(`Type: ${cat.isIndoor()}`);
  results.push(`Says: ${cat.speak()}`);

  // Static methods
  results.push(`3 + 5 = ${MathHelper.add(3, 5)}`);
  results.push(`4 * 6 = ${MathHelper.multiply(4, 6)}`);
  results.push(`PI = ${MathHelper.PI}`);
  results.push(`Circle(5) area = ${MathHelper.circleArea(5).toFixed(2)}`);

  // Multiple instances
  const counter2 = new Counter(100);
  results.push(`counter1: ${counter.getCount()}, counter2: ${counter2.getCount()}`);

  // Constructor overloading simulation
  const animals: Animal[] = [
    new Dog('Rex', 'German Shepherd'),
    new Cat('Mittens'),
    new Dog('Max'),
  ];

  for (const animal of animals) {
    results.push(`${animal.getName()}: ${animal.speak()}`);
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Class Component Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
