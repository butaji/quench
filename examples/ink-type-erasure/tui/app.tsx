// ink-type-erasure example — demonstrates TypeScript type-level features that are erased at runtime.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// These TypeScript constructs are erased and have zero runtime impact:
// - Generic type parameters
// - Mapped types
// - Conditional types
// - Utility types (Partial, Required, Pick, Omit, etc.)
// - Type aliases

import React from 'react';
import { Box, Text } from 'ink';

// Generic type parameter - erased at runtime
function identity<T>(x: T): T {
  return x;
}

// Generic class
class Container<T> {
  constructor(public value: T) {}
  get(): T {
    return this.value;
  }
}

// Mapped type - all properties become optional strings
type Stringify<T> = {
  [K in keyof T]: string;
};

// Conditional type - erased at runtime
type IsArray<T> = T extends any[] ? true : false;

// Utility types
interface Config {
  name: string;
  age: number;
  active: boolean;
}

type PartialConfig = Partial<Config>;
type RequiredConfig = Required<Config>;
type PickConfig = Pick<Config, 'name' | 'age'>;
type OmitConfig = Omit<Config, 'active'>;

// Intersection types
type Employee = { employeeId: number } & Config;
type Manager = { department: string } & Employee;

// Union types
type StringOrNumber = string | number;
type Callback<T> = (value: T) => void;

// Indexed access types
type ConfigName = Config['name'];
type ConfigKeys = keyof Config;

export default function TypeErasureDemo() {
  const results: string[] = [];

  // Generic function
  const idStr = identity<string>('hello');
  const idNum = identity<number>(42);
  results.push(`identity('hello'): ${idStr}`);
  results.push(`identity(42): ${idNum}`);

  // Generic class
  const container = new Container('test');
  results.push(`Container.get(): ${container.get()}`);

  // Mapped type usage
  const mapped: Stringify<{ x: number; y: number }> = { x: '1', y: '2' };
  results.push(`Stringify: x=${mapped.x}, y=${mapped.y}`);

  // Conditional type
  const isArrString = identity<IsArray<string>>('whatever' as any);
  const isArrNumber = identity<IsArray<number[]>>([] as any);
  results.push(`IsArray<string>: ${isArrString}`);
  results.push(`IsArray<number[]>: ${isArrNumber}`);

  // Utility types
  const partial: PartialConfig = { name: 'John' };
  const pick: PickConfig = { name: 'Jane', age: 30 };
  const omit: OmitConfig = { name: 'Bob', age: 25 };
  results.push(`Partial: ${partial.name || 'undefined'}`);
  results.push(`Pick: name=${pick.name}, age=${pick.age}`);
  results.push(`Omit: name=${omit.name}, age=${omit.age}`);

  // Intersection type
  const employee: Employee = { employeeId: 1, name: 'Alice', age: 28, active: true };
  const manager: Manager = { employeeId: 2, name: 'Bob', age: 35, active: true, department: 'Engineering' };
  results.push(`Employee: ${employee.name} (#${employee.employeeId})`);
  results.push(`Manager: ${manager.name}, ${manager.department}`);

  // Indexed access
  const keys: ConfigKeys[] = ['name', 'age', 'active'];
  results.push(`Config keys: ${keys.join(', ')}`);

  // Union type
  const strings: StringOrNumber[] = ['hello', 42, 'world', 100];
  const stringParts: string[] = strings.filter((s): s is string => typeof s === 'string');
  const numbers: number[] = strings.filter((n): n is number => typeof n === 'number');
  results.push(`Strings: ${stringParts.join(', ')}`);
  results.push(`Numbers: ${numbers.join(', ')}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Type Erasure Demo</Text>
      <Text dimColor>All TypeScript types are erased at runtime</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
