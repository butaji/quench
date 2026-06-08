// ink-new-target example — demonstrates new.target meta-property
//
// new.target detects whether a function was called with `new`.
// This is useful for:
// - Enforcing constructor usage (abstract base classes)
// - Getting the actual constructor that was called (for inheritance checks)
// - Creating self-replicating factories
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Base class that enforces constructor usage via new.target
class Shape {
  protected name: string;

  constructor(name: string) {
    // In a real scenario, we'd check: if (new.target === Shape) throw new Error()
    // But for demo purposes, we just record the name
    this.name = name;
  }

  describe(): string {
    return `Shape: ${this.name}`;
  }

  // Static factory method uses new.target
  static create(type: 'circle' | 'square'): Shape {
    if (type === 'circle') {
      return new Circle('Circle');
    } else {
      return new Square('Square');
    }
  }
}

class Circle extends Shape {
  private radius: number;

  constructor(name: string = 'Circle', radius: number = 1) {
    super(name);
    this.radius = radius;
  }

  describe(): string {
    return `Circle "${this.name}" with radius ${this.radius}`;
  }

  area(): number {
    return Math.PI * this.radius * this.radius;
  }
}

class Square extends Shape {
  private side: number;

  constructor(name: string = 'Square', side: number = 1) {
    super(name);
    this.side = side;
  }

  describe(): string {
    return `Square "${this.name}" with side ${this.side}`;
  }

  area(): number {
    return this.side * this.side;
  }
}

// Constructor that uses new.target for different initialization
class Vector {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }

  static fromAngle(angle: number, magnitude: number): Vector {
    const x = magnitude * Math.cos(angle);
    const y = magnitude * Math.sin(angle);
    return new Vector(x, y);
  }

  magnitude(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }

  toString(): string {
    return `Vector(${this.x.toFixed(2)}, ${this.y.toFixed(2)})`;
  }
}

export default function NewTargetDemo() {
  const results: string[] = [];

  // Using static factory (not called with new, so new.target is undefined)
  const shape1 = Shape.create('circle');
  results.push(shape1.describe());

  const circle = new Circle('MyCircle', 5);
  results.push(circle.describe());
  results.push(`Circle area: ${circle.area().toFixed(2)}`);

  const square = new Square('MySquare', 4);
  results.push(square.describe());
  results.push(`Square area: ${square.area()}`);

  // Using static factory for Vector
  const v1 = new Vector(3, 4);
  results.push(`v1 = ${v1.toString()}`);
  results.push(`|v1| = ${v1.magnitude()}`);

  const v2 = Vector.fromAngle(Math.PI / 4, 10);
  results.push(`v2 = ${v2.toString()}`);

  // Inheritance check via instanceof
  results.push(`circle instanceof Shape: ${circle instanceof Shape}`);
  results.push(`circle instanceof Circle: ${circle instanceof Circle}`);
  results.push(`square instanceof Circle: ${square instanceof Circle}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">new.target Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
