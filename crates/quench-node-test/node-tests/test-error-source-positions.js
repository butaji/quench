'use strict';
const assert = require('assert');
const { internalBinding } = require('internal/test/binding');
const binding = internalBinding('errors');
assert.strictEqual(binding, internalBinding('errors'));
const get = binding.getErrorSourcePositions;
assert.strictEqual(get.name, 'getErrorSourcePositions');
assert.strictEqual(get.length, 0);
assert.strictEqual(Object.hasOwn(get, 'prototype'), false);
function factory() { return new Error('sample'); }
function check(position, source) {
  assert.strictEqual(Object.getPrototypeOf(position), null);
  assert.deepStrictEqual(Object.keys(position), ['sourceLine', 'scriptResourceName', 'lineNumber', 'startColumn']);
  assert.strictEqual(position.sourceLine, source);
  assert.strictEqual(position.startColumn, source.indexOf('new Error'));
  assert.strictEqual(typeof position.lineNumber, 'number');
  assert.strictEqual(typeof position.scriptResourceName, 'string');
}
const source = "function factory() { return new Error('sample'); }";
const error = factory();
check(get(error), source);
let reads = 0;
Object.defineProperty(error, 'stack', { configurable: true, get() { reads++; throw Error('unexpected stack read'); } });
check(get(error), source);
assert.strictEqual(reads, 0);
Object.defineProperty(error, 'stack', { value: 'changed', configurable: true });
check(get(error), source);
function capture() { const result = {}; Error.captureStackTrace(result, capture); return result; }
const captured = capture();
assert.strictEqual(get(captured).sourceLine, 'const captured = capture();');
assert.strictEqual(get(captured).startColumn, 'const captured = '.length);
const evalError = eval('new Error("from eval")');
const evalPosition = get(evalError);
assert.strictEqual(evalPosition.sourceLine, 'new Error("from eval")');
assert.strictEqual(evalPosition.scriptResourceName, undefined);
assert.strictEqual(evalPosition.lineNumber, 1);
assert.strictEqual(evalPosition.startColumn, 0);
if (global.gc) { gc(); gc(); }
check(get(error), source);
const proxy = new Proxy(error, { get() { throw Error('unexpected get'); }, getPrototypeOf() { throw Error('unexpected prototype'); } });
function fallback(value) { return get(value); }
const fallbackSource = 'function fallback(value) { return get(value); }';
for (const value of [{}, proxy]) {
  const position = fallback(value);
  assert.strictEqual(position.sourceLine, fallbackSource);
  assert.strictEqual(position.startColumn, fallbackSource.indexOf('get(value)'));
}
const formatted = factory();
void formatted.stack;
assert.strictEqual(fallback(formatted).sourceLine, fallbackSource);
const limit = Error.stackTraceLimit;
for (const value of [0, undefined]) {
  Error.stackTraceLimit = value;
  assert.strictEqual(fallback(factory()).sourceLine, fallbackSource);
}
Error.stackTraceLimit = limit;
let assertion;
try { assert.ok(false); } catch (error) { assertion = error; }
assert.strictEqual(assertion.code, 'ERR_ASSERTION');
assert.strictEqual(assertion.message, 'The expression evaluated to a falsy value:\n\n  assert.ok(false)\n');
console.log('error-source-positions: ok');
