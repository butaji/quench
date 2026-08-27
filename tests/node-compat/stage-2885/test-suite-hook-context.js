const assert = require('assert');
const { describe, before, after, it, getTestContext } = require('node:test');

describe('suite', () => {
  before(() => assert.strictEqual(getTestContext().name, 'suite'));
  after(() => assert.strictEqual(getTestContext().name, 'suite'));
  it('child', () => assert.strictEqual(getTestContext().name, 'child'));
});
