// ink-reflect-api example — demonstrates Reflect API
//
// Reflect is an ES2015 built-in object that provides methods for
// interceptable JavaScript operations. These methods are the same as
// those of proxy handlers.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

const obj = { name: 'App', version: 1 };

// Reflect.get - gets a property of an object
const name = Reflect.get(obj, 'name');

// Reflect.set - sets a property of an object
Reflect.set(obj, 'version', 2);

// Reflect.defineProperty before ownKeys
Reflect.defineProperty(obj, 'author', { value: 'Developer', writable: true });

// Reflect.ownKeys - returns all own property keys
const keys = Reflect.ownKeys(obj);

// Reflect.has - checks if a property exists
const hasName = Reflect.has(obj, 'name');
const hasId = Reflect.has(obj, 'id');
const hasAuthor = Reflect.has(obj, 'author');

// Reflect.getPrototypeOf - returns the prototype
const proto = Reflect.getPrototypeOf(obj);

// Reflect.isExtensible - checks if object is extensible
const isExtensible = Reflect.isExtensible(obj);

// Reflect.deleteProperty - deletes a property
const testObj = { x: 10, y: 20 };
const deleted = Reflect.deleteProperty(testObj, 'y');

// Reflect.preventExtensions - prevents extensions
const sealed = { z: 1 };
Reflect.preventExtensions(sealed);
const sealedExtensible = Reflect.isExtensible(sealed);

export default function ReflectDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Reflect API Demo</Text>
      <Text></Text>
      <Text>Object: name={obj.name}, version={obj.version}</Text>
      <Text>Reflect.get(obj, 'name'): {name}</Text>
      <Text>Reflect.has(obj, 'name'): {hasName ? 'true' : 'false'}</Text>
      <Text>Reflect.has(obj, 'id'): {hasId ? 'true' : 'false'}</Text>
      <Text>Reflect.has(obj, 'author'): {hasAuthor ? 'true' : 'false'}</Text>
      <Text>Reflect.ownKeys(obj): {keys.join(', ')}</Text>
      <Text>Reflect.getPrototypeOf(obj): {proto?.constructor?.name || 'null'}</Text>
      <Text>Reflect.isExtensible(obj): {isExtensible ? 'true' : 'false'}</Text>
      <Text>Reflect.deleteProperty(testObj, 'y'): {deleted ? 'true' : 'false'}</Text>
      <Text>Reflect.preventExtensions: sealed={sealedExtensible ? 'true' : 'false'}</Text>
    </Box>
  );
}
