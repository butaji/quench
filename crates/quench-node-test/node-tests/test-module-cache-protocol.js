'use strict';
const assert=require('assert');
const { internalBinding }=require('internal/test/binding');
const binding=internalBinding('modules');
const types=binding.cachedCodeTypes;
assert.strictEqual(Object.getPrototypeOf(types),null);
assert.strictEqual(internalBinding('modules').cachedCodeTypes,types);
console.log(JSON.stringify([Object.entries(types),Object.entries(Object.getOwnPropertyDescriptors(types)).map(([name,d])=>[name,d.value,d.writable,d.enumerable,d.configurable]),binding.compileCacheStatus]));
