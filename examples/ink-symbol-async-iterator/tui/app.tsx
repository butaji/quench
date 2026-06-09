// Symbol.asyncIterator example — async iteration protocol
//
// Exercises:
//   - *[Symbol.asyncIterator]() async generator method
//   - for await...of with async iterable
//   - AsyncIterator interface
//
// All three environments must produce the same look:
//   1. deno (real Ink) - full functionality
//   2. runts dev (rquickjs engine) - full functionality
//   3. runts build (codegen->runts-ink) - compiles correctly, shows static state

import React, { useState, useEffect } from 'react';
import { Box, Text, Newline } from 'ink';

// Simple async iterator class
class AsyncCounter {
  private count = 0;

  async *[Symbol.asyncIterator]() {
    while (this.count < 3) {
      yield ++this.count;
    }
  }
}

// Async iterator function that yields strings
async function* asyncGreetings(): AsyncIterableIterator<string> {
  yield 'Hello';
  yield 'Async';
  yield 'World';
}

// Pre-computed results for compile path display
const counterResult = [1, 2, 3].join(', ');
const greetingsResult = ['Hello', 'Async', 'World'].join(', ');

export default function AsyncIteratorDemo() {
  const [counterItems, setCounterItems] = useState<string[]>([]);
  const [greetingItems, setGreetingItems] = useState<string[]>([]);

  useEffect(() => {
    // Collect async iterator results
    const collect = async () => {
      // AsyncCounter iteration
      const counter = new AsyncCounter();
      const counterResults: string[] = [];
      for await (const n of counter) {
        counterResults.push(String(n));
      }
      setCounterItems(counterResults);

      // async generator function iteration
      const greetingResults: string[] = [];
      for await (const greeting of asyncGreetings()) {
        greetingResults.push(greeting);
      }
      setGreetingItems(greetingResults);
    };
    collect();
  }, []);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="magenta">Symbol.asyncIterator Demo</Text>
      <Newline />
      <Text>AsyncCounter: </Text>
      <Text>{counterItems.length > 0 ? counterItems.join(', ') : counterResult}</Text>
      <Newline />
      <Text>asyncGreetings: </Text>
      <Text>{greetingItems.length > 0 ? greetingItems.join(', ') : greetingsResult}</Text>
      <Newline />
      <Text dimColor>*[Symbol.asyncIterator](), for await...of work.</Text>
    </Box>
  );
}
