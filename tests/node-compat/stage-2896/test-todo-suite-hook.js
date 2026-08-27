const { describe, before, it } = require('node:test');

describe('todo suite', { todo: 'advisory' }, () => {
  before(() => { throw new Error('advisory hook failure'); });
  it('child one', () => {});
  it('child two', () => {});
});
