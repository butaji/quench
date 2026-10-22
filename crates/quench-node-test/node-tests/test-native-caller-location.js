'use strict';
const assert = require('assert');
const { internalBinding } = require('internal/test/binding');
const binding = internalBinding('util');
const get = binding.getCallerLocation;
assert.strictEqual(binding, internalBinding('util'));
assert.strictEqual(get.name, 'getCallerLocation');
assert.strictEqual(get.length, 0);
assert.strictEqual(Object.hasOwn(get, 'prototype'), false);
function capture() { return get(); }
function user() { return capture(); }
Object.defineProperty(capture, 'name', { get() { throw Error('name getter'); } });
const before = user();
assert.strictEqual(Array.isArray(before), true);
assert.strictEqual(before.length, 3);
assert.strictEqual(typeof before[0], 'number');
assert.strictEqual(before[1], 'function user() { return '.length + 1);
assert.strictEqual(before[2], __filename);
Error.stackTraceLimit = 0;
Object.defineProperty(Error, 'prepareStackTrace', { configurable: true, get() { throw Error('stack hook'); } });
const after = user();
assert.deepStrictEqual(after, before);
assert.strictEqual(eval('(function outer(){return (function inner(){return get()})()})()'), undefined);
console.log(JSON.stringify(before));
console.log('native-caller-location: ok');
