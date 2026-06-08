// ink-mapped-types example — demonstrates mapped types
//
// Mapped types are a powerful TypeScript feature that create new types
// by iterating over the keys of an existing type. They are purely
// type-level and get erased at compile time.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

interface User {
  name: string;
  age: number;
  active: boolean;
}

// Basic mapped types
type NullableUser = { [K in keyof User]: User[K] | null };
type UserStrings = { [K in keyof User]: string };
type OptionalUser = { [K in keyof User]?: User[K] };
type ReadonlyUser = { readonly [K in keyof User]: User[K] };

// Partial and Required
type PartialUser = Partial<User>;
type RequiredUser = Required<User>;

// Pick and Omit
type UserName = Pick<User, 'name'>;
type UserWithoutAge = Omit<User, 'age'>;

// Record
type Role = 'admin' | 'user' | 'guest';
type RolePermissions = Record<Role, string[]>;

// keyof with generic function
function keysOf<T extends object>(obj: T): (keyof T)[] {
  return Object.keys(obj) as (keyof T)[];
}

function valuesOf<T extends object>(obj: T): T[keyof T][] {
  return Object.values(obj) as T[keyof T][];
}

function entriesOf<T extends object>(obj: T): [keyof T, T[keyof T]][] {
  return Object.entries(obj) as [keyof T, T[keyof T]][];
}

// Filter mapped type (conditional in mapped)
type NonBoolean<T> = { [K in keyof T]: T[K] extends boolean ? never : T[K] };

export default function MappedTypesDemo() {
  const user: User = { name: 'Alice', age: 30, active: true };
  const nullable: NullableUser = { name: 'Bob', age: null, active: true };
  const partial: OptionalUser = { name: 'Carol' };
  const keys = keysOf(user);
  const vals = valuesOf(user);
  const entries = entriesOf(user);
  const roles: RolePermissions = {
    admin: ['read', 'write', 'delete'],
    user: ['read', 'write'],
    guest: ['read'],
  };

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Mapped Types Demo</Text>
      <Text></Text>
      <Text>Original user:</Text>
      <Text>  name: {user.name}</Text>
      <Text>  age: {user.age}</Text>
      <Text>  active: {user.active ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>Nullable user:</Text>
      <Text>  name: {nullable.name}</Text>
      <Text>  age: {nullable.age}</Text>
      <Text></Text>
      <Text>Optional partial:</Text>
      <Text>  name: {partial.name ?? 'N/A'}</Text>
      <Text></Text>
      <Text>keysOf(user):</Text>
      <Text>  {keys.join(', ')}</Text>
      <Text></Text>
      <Text>valuesOf(user):</Text>
      <Text>  {vals.join(', ')}</Text>
      <Text></Text>
      <Text>entriesOf(user):</Text>
      {entries.map(([k, v]) => (
        <Text key={k as string}>  {k as string}: {v as string}</Text>
      ))}
      <Text></Text>
      <Text>Role permissions:</Text>
      <Text>  admin: {roles.admin.join(', ')}</Text>
      <Text>  user: {roles.user.join(', ')}</Text>
      <Text>  guest: {roles.guest.join(', ')}</Text>
    </Box>
  );
}
