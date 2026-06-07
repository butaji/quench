// ink-getter-setter example — demonstrates getters and setters in classes.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Simple class with getter and setter
class Counter {
  private _value: number;
  private _name: string;

  constructor(initial: number = 0, name: string = 'Counter') {
    this._value = initial;
    this._name = name;
  }

  // Getter for value
  get value(): number {
    return this._value;
  }

  // Setter for value
  set value(v: number) {
    this._value = v;
  }

  // Computed getter
  get doubled(): number {
    return this._value * 2;
  }

  // Getter with string
  get label(): string {
    return `${this._name}: ${this._value}`;
  }

  // Read-only property (only getter, no setter)
  get isPositive(): boolean {
    return this._value > 0;
  }
}

// Class demonstrating multiple getters
class Rectangle {
  private _width: number;
  private _height: number;

  constructor(width: number, height: number) {
    this._width = width;
    this._height = height;
  }

  get width(): number { return this._width; }
  get height(): number { return this._height; }
  set width(w: number) { this._width = w; }
  set height(h: number) { this._height = h; }

  get area(): number { return this._width * this._height; }
  get perimeter(): number { return 2 * (this._width + this._height); }
  get isSquare(): boolean { return this._width === this._height; }
}

export default function GetterSetterDemo() {
  const results: string[] = [];

  // Basic getter/setter usage
  const counter = new Counter(10, 'Basic');
  results.push(`Initial: ${counter.value}`);
  results.push(`Label: ${counter.label}`);
  results.push(`Doubled: ${counter.doubled}`);
  results.push(`Is Positive: ${counter.isPositive}`);

  // Modify via setter
  counter.value = 25;
  results.push(`After set to 25: ${counter.value}`);
  results.push(`Doubled now: ${counter.doubled}`);
  results.push(`Is Positive now: ${counter.isPositive}`);

  // Reset via setter
  counter.value = 0;
  results.push(`After reset to 0: ${counter.value}`);
  results.push(`Is Positive now: ${counter.isPositive}`);

  // Rectangle example
  const rect = new Rectangle(5, 3);
  results.push('');
  results.push(`Rectangle ${rect.width}x${rect.height}:`);
  results.push(`  Area: ${rect.area}`);
  results.push(`  Perimeter: ${rect.perimeter}`);
  results.push(`  Is Square: ${rect.isSquare}`);

  // Modify rectangle
  rect.width = 4;
  rect.height = 4;
  results.push(`After making square 4x4:`);
  results.push(`  Area: ${rect.area}`);
  results.push(`  Is Square: ${rect.isSquare}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Getter/Setter Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
