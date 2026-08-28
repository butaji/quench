'use strict';

const assert = require('assert');
const dc = require('diagnostics_channel');
const { AsyncLocalStorage } = require('async_hooks');

const bounded = dc.boundedChannel('stage-3000-scope');
const store = new AsyncLocalStorage();
const events = [];
bounded.start.bindStore(store, (context) => context.value);
bounded.subscribe({
  start: (context) => events.push(['start', context.value, store.getStore()]),
  end: (context) => events.push(['end', context.value, store.getStore()]),
});
const context = { value: 'during' };
{
  using scope = bounded.withScope(context);
  assert.strictEqual(store.getStore(), 'during');
  context.value = 'after-start';
}
assert.deepStrictEqual(events, [
  ['start', 'during', 'during'],
  ['end', 'after-start', 'during'],
]);
assert.strictEqual(store.getStore(), undefined);
