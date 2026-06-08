// ink-infer-conditional example — demonstrates infer in conditional types
//
// The 'infer' keyword in TypeScript (TS 2.8) is used within conditional
// type declarations to extract and name a type variable from a matched
// position. This enables powerful type manipulation.
//
// All 'infer' usage is purely type-level and gets erased at compile time,
// so there is no runtime impact.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Extract return type using infer
type ReturnType<T> = T extends (...args: any[]) => infer R ? R : never;

function getGreeting(): string {
  return 'Hello';
}

function getAge(): number {
  return 30;
}

function getUser(): { name: string; age: number } {
  return { name: 'Alice', age: 25 };
}

type GreetingType = ReturnType<typeof getGreeting>;
type AgeType = ReturnType<typeof getAge>;
type UserType = ReturnType<typeof getUser>;

const greeting: GreetingType = getGreeting();
const age: AgeType = getAge();
const user: UserType = getUser();

// Extract first element type from array using infer
type First<T> = T extends [infer F, ...any[]] ? F : never;
type ArrayFirst = First<[string, number, boolean]>;

const first: ArrayFirst = 'first-element';

// Extract element type from Promise
type Awaited<T> = T extends Promise<infer U> ? U : T;
type PromiseString = Promise<string>;
type ResolvedString = Awaited<PromiseString>;

const resolved: ResolvedString = 'resolved-value';

// Extract property type using infer
type PropType<T, K extends keyof T> = T[K];
type NamePropType = PropType<{ name: string; age: number }, 'name'>;

const nameProp: NamePropType = 'name-prop';

// Extract constructor type from class
type ConstructorArg<T> = T extends new (...args: infer A) => any ? A : never;
type ConstructorFirstArg = ConstructorArg<new (name: string, age: number) => void>;

const firstArg: ConstructorFirstArg = 'constructor-arg';

export default function InferConditionalDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">infer in Conditional Types Demo</Text>
      <Text></Text>
      <Text>ReturnType extraction:</Text>
      <Text>  greeting: {greeting}</Text>
      <Text>  age: {age}</Text>
      <Text>  user: {JSON.stringify(user)}</Text>
      <Text></Text>
      <Text>First element extraction:</Text>
      <Text>  first: {first}</Text>
      <Text></Text>
      <Text>Promise await extraction:</Text>
      <Text>  resolved: {resolved}</Text>
      <Text></Text>
      <Text>Property type extraction:</Text>
      <Text>  nameProp: {nameProp}</Text>
      <Text></Text>
      <Text>Constructor arg extraction:</Text>
      <Text>  firstArg: {firstArg}</Text>
    </Box>
  );
}
