// ink-utility-types example — demonstrates TypeScript's built-in utility types.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Utility types are erased at compile time. They have no runtime
// impact on the generated JavaScript or Rust code.

import React from 'react';
import { Box, Text } from 'ink';

// Base interface for demos
interface User {
  name: string;
  age: number;
  email?: string;
  active: boolean;
}

// --- Partial<T> ---
type PartialUser = Partial<User>;
const partialUser: PartialUser = { name: 'Alice' };

// --- Required<T> ---
type RequiredUser = Required<User>;

// --- Readonly<T> ---
type ReadonlyUser = Readonly<User>;
const readonlyUser: ReadonlyUser = {
  name: 'Bob',
  age: 25,
  email: 'bob@example.com',
  active: true,
};

// --- Pick<T, K> ---
type UserPreview = Pick<User, 'name' | 'age'>;
const preview: UserPreview = { name: 'Carol', age: 30 };

// --- Omit<T, K> ---
type UserWithoutEmail = Omit<User, 'email'>;
const noEmail: UserWithoutEmail = { name: 'Dave', age: 35, active: false };

// --- Record<K, T> ---
type Status = 'idle' | 'loading' | 'done' | 'error';
const statusLabels: Record<Status, string> = {
  idle: 'Waiting',
  loading: 'Loading...',
  done: 'Complete',
  error: 'Failed',
};
const userRecord: Record<string, UserPreview> = {
  alice: preview,
  carol: { name: 'Carol', age: 30 },
};

// --- Exclude<T, U> ---
type StringOrNumber = string | number | boolean;
type NoBoolean = Exclude<StringOrNumber, boolean>;

// --- Extract<T, U> ---
type OnlyStrings = Extract<StringOrNumber, string>;

// --- NonNullable<T> ---
type MaybeString = string | null | undefined | void;
type DefinitelyString = NonNullable<MaybeString>;

// --- Parameters<T> ---
function greet(name: string, age: number): string {
  return `Hello, ${name}! You are ${age} years old.`;
}
type GreetParams = Parameters<typeof greet>;
type FirstParam = GreetParams[0];

// --- ReturnType<T> ---
type GreetReturn = ReturnType<typeof greet>;

// --- ConstructorParameters<T> ---
class Person {
  constructor(public name: string, public age: number) {}
}
type PersonCtorParams = ConstructorParameters<typeof Person>;

// --- InstanceType<T> ---
type PersonInstance = InstanceType<typeof Person>;

// --- Awaited<T> ---
type PromiseOfString = Promise<string>;
type Unwrapped = Awaited<PromiseOfString>;

// --- Chained utility types ---
type RequiredReadonlyUser = Required<Readonly<User>>;

// --- Function for runtime demo ---
function processUser(u: User): string {
  return `${u.name} (${u.age})`;
}

export default function UtilityTypesDemo() {
  const results: string[] = [];

  // Partial
  results.push(`PartialUser.name: ${partialUser.name ?? 'undefined'}`);

  // Readonly
  results.push(`ReadonlyUser: ${readonlyUser.name}, ${readonlyUser.age}`);

  // Pick
  results.push(`UserPreview: ${preview.name}, ${preview.age}`);

  // Omit
  results.push(`UserWithoutEmail: ${noEmail.name}, ${noEmail.active}`);

  // Record
  results.push(`statusLabels.done: ${statusLabels.done}`);
  results.push(`userRecord.alice: ${userRecord.alice?.name}`);

  // Exclude/Extract/NonNullable
  const noBool: NoBoolean = 'test';
  const onlyStr: OnlyStrings = 'hello';
  const defStr: DefinitelyString = 'value';
  results.push(`NoBoolean: ${noBool}`);
  results.push(`OnlyStrings: ${onlyStr}`);
  results.push(`DefinitelyString: ${defStr}`);

  // Parameters/ReturnType
  const params: GreetParams = ['World', 42];
  const ret: GreetReturn = greet(...params);
  results.push(`GreetParams: [${params.join(', ')}]`);
  results.push(`ReturnType: ${ret}`);

  // ConstructorParameters/InstanceType
  const ctorParams: PersonCtorParams = ['Alice', 30];
  results.push(`PersonCtorParams: [${ctorParams.join(', ')}]`);

  // Awaited
  const unwrapped: Unwrapped = 'async value';
  results.push(`Awaited: ${unwrapped}`);

  // Chained
  const chained: RequiredReadonlyUser = {
    name: 'Charlie',
    age: 28,
    email: 'charlie@example.com',
    active: true,
  };
  results.push(`Chained utility: ${chained.name}`);

  // Function calls
  results.push(`processUser: ${processUser(readonlyUser)}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">TypeScript Utility Types Demo</Text>
      <Text dimColor>All utility types erased at compile time</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
