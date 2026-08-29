'use strict';

const assert = require('assert');
const dc = require('diagnostics_channel');
const { AsyncLocalStorage } = require('async_hooks');

const channel = dc.channel('stage-2999-run-stores');
const store = new AsyncLocalStorage();
const seen = [];
channel.bindStore(store);
channel.subscribe((message) => seen.push([message, store.getStore()]));
const thisArg = { marker: true };
const result = channel.runStores({ value: 1 }, function (arg) {
  assert.strictEqual(this, thisArg);
  assert.strictEqual(arg, 2);
  assert.deepStrictEqual(store.getStore(), { value: 1 });
  return 3;
}, thisArg, 2);
assert.strictEqual(result, 3);
assert.deepStrictEqual(seen, [[{ value: 1 }, { value: 1 }]]);
assert.strictEqual(store.getStore(), undefined);
