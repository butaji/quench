'use strict';

const assert = require('node:assert/strict');
const { describe, it, mock } = require('node:test');

describe('node:test surface', () => {
  it('runs nested tests and records mocks', () => {
    const callback = mock.fn((value) => value + 1);
    assert.strictEqual(callback(2), 3);
    assert.strictEqual(callback.mock.calls.length, 1);
    assert.deepStrictEqual(callback.mock.calls[0].arguments, [2]);
  });
});
