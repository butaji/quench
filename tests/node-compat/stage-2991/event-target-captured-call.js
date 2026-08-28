const assert = require('assert');
const { EventTarget } = require('internal/event_target');
const target = new EventTarget();
let calls = 0;
const sink = () => { calls++; };
target.addEventListener('ready', () => sink());
target.dispatchEvent(new Event('ready'));
assert.strictEqual(calls, 1);
