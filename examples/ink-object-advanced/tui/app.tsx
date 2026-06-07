// ink-object-advanced example — demonstrates getters, setters, computed keys,
// method shorthand, and object expressions.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function ObjectAdvancedDemo() {
  const results: string[] = [];

  // Object with method shorthand
  const counter = {
    value: 0,
    increment() {
      this.value += 1;
      return this.value;
    },
    decrement() {
      this.value -= 1;
      return this.value;
    },
  };
  counter.increment();
  counter.increment();
  counter.decrement();
  results.push(`counter.value: ${counter.value}`);

  // Computed property keys
  const key = 'dynamicKey';
  const obj = {
    normalKey: 'normal',
    [key]: 'computed',
    ['prefix_' + key]: 'concatenated',
  };
  results.push(`obj.normalKey: ${obj.normalKey}`);
  results.push(`obj[key]: ${obj[key]}`);
  results.push(`obj['prefix_' + key]: ${obj['prefix_' + key]}`);

  // Getter and setter
  let internalTemp = 20;
  const thermometer = {
    get temperature() {
      return internalTemp;
    },
    set temperature(value: number) {
      internalTemp = Math.max(-50, Math.min(50, value));
    },
  };
  results.push(`initial temp: ${thermometer.temperature}`);
  thermometer.temperature = 30;
  results.push(`after setting to 30: ${thermometer.temperature}`);
  thermometer.temperature = 100; // should clamp to 50
  results.push(`after setting to 100 (clamped): ${thermometer.temperature}`);

  // Shorthand property names
  const x = 10;
  const y = 20;
  const point = { x, y };
  results.push(`shorthand: (${point.x}, ${point.y})`);

  // Object method with computed key
  const methods = {
    ['add'](a: number, b: number) {
      return a + b;
    },
    ['subtract'](a: number, b: number) {
      return a - b;
    },
  };
  results.push(`methods.add(5, 3): ${methods['add'](5, 3)}`);
  results.push(`methods.subtract(10, 4): ${methods['subtract'](10, 4)}`);

  // Nested object with methods
  const bankAccount = {
    balance: 100,
    deposit(amount: number) {
      this.balance += amount;
      return this.balance;
    },
    withdraw(amount: number) {
      this.balance -= amount;
      return this.balance;
    },
  };
  bankAccount.deposit(50);
  bankAccount.withdraw(30);
  results.push(`bank balance after +50 -30: ${bankAccount.balance}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Object Advanced Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
