'use strict';

const assert = require('assert');
const { internalBinding } = require('internal/test/binding');
const binding = internalBinding('timers');

for (const name of ['scheduleTimer', 'toggleTimerRef', 'toggleImmediateRef']) {
  assert.strictEqual(typeof binding[name], 'function');
}
binding.scheduleTimer(1);
binding.toggleTimerRef(true);
binding.toggleTimerRef(false);
binding.toggleImmediateRef(true);
binding.toggleImmediateRef(false);
