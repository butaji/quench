// ink-object-meta example — demonstrates Object meta-level methods.
//
// Object meta-methods are fundamental JavaScript runtime features:
//   - Object.create(proto) — creates object with given prototype
//   - Object.defineProperty(obj, prop, descriptor) — define property with descriptors
//   - Object.freeze(obj) — make object completely immutable
//   - Object.seal(obj) — prevent add/remove properties
//   - Object.assign(target, ...sources) — copy properties from sources
//
// In the compile path, these are compiled to Rust runtime calls.

import React from 'react';
import { Box, Text } from 'ink';

// Object.create — creates object with given prototype chain
const proto = { greet: 'Hello from prototype', value: 42 };
const created = Object.create(proto);
created.ownProp = 'own value';
created.nested = { inner: 'nested value' };

// Object.defineProperty — define with descriptor
const defined: any = {};
Object.defineProperty(defined, 'readonlyProp', {
  value: 'read only',
  writable: false,
  enumerable: true,
  configurable: false,
});
Object.defineProperty(defined, 'getterProp', {
  get() { return 'from getter'; },
  enumerable: true,
});

// Object.freeze — make object completely immutable
const frozen = Object.freeze({ a: 1, b: 2, nested: { x: 10 } });
const frozenArr = Object.freeze([1, 2, 3]);

// Object.seal — prevent add/remove (but allow modifications)
const sealed: any = { x: 1, y: 2 };
Object.seal(sealed);

// Object.assign — copy properties
const assigned = Object.assign({ a: 1 }, { b: 2 }, { c: 3 }, { a: 10 });

// Object.preventExtensions
const extensible: any = { a: 1 };
Object.preventExtensions(extensible);

export default function ObjectMetaDemo() {
  // Cache all values to ensure they're computed
  const createGreet = (created as any).greet;
  const createValue = (created as any).value;
  const createOwn = (created as any).ownProp;
  const createNested = (created as any).nested?.inner;
  const defReadonly = defined.readonlyProp;
  const defGetter = defined.getterProp;
  const frozenA = frozen.a;
  const frozenB = frozen.b;
  const frozenNested = frozen.nested.x;
  const isFrozenVal = Object.isFrozen(frozen);
  const frozenArr0 = frozenArr[0];
  const sealedX = sealed.x;
  const sealedY = sealed.y;
  const isSealedVal = Object.isSealed(sealed);
  const assignA = assigned.a;
  const assignB = assigned.b;
  const assignC = assigned.c;
  const extA = extensible.a;
  const isExtVal = Object.isExtensible(extensible);
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Object Meta-Methods</Text>
      <Text dimColor>create, defineProperty, freeze, seal, assign</Text>
      <Text></Text>
      <Text>--- Object.create ---</Text>
      <Text>prototype.greet: {createGreet}</Text>
      <Text>prototype.value: {createValue}</Text>
      <Text>ownProp: {createOwn}</Text>
      <Text>nested.inner: {createNested}</Text>
      <Text></Text>
      <Text>--- Object.defineProperty ---</Text>
      <Text>readonlyProp: {defReadonly}</Text>
      <Text>getterProp: {defGetter}</Text>
      <Text></Text>
      <Text>--- Object.freeze ---</Text>
      <Text>frozen: a={frozenA}, b={frozenB}</Text>
      <Text>nested.x: {frozenNested}</Text>
      <Text>isFrozen: {isFrozenVal ? 'true' : 'false'}</Text>
      <Text>frozenArr[0]: {frozenArr0}</Text>
      <Text></Text>
      <Text>--- Object.seal ---</Text>
      <Text>sealed: x={sealedX}, y={sealedY}</Text>
      <Text>isSealed: {isSealedVal ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>--- Object.assign ---</Text>
      <Text>assigned: a={assignA}, b={assignB}, c={assignC}</Text>
      <Text></Text>
      <Text>--- Object.preventExtensions ---</Text>
      <Text>extensible: a={extA}</Text>
      <Text>isExtensible: {isExtVal ? 'true' : 'false'}</Text>
    </Box>
  );
}
