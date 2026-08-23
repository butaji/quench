const assert = require('assert');
const { parseArgs } = require('util');

const result = parseArgs({
  args: ['--name=quench', '--verbose', '-c', '2', 'input.js'],
  options: {
    name: { type: 'string' },
    verbose: { type: 'boolean' },
    count: { type: 'string', short: 'c' }
  },
  allowPositionals: true
});
assert.strictEqual(result.values.name, 'quench');
assert.strictEqual(result.values.verbose, true);
assert.strictEqual(result.values.count, '2');
assert.deepStrictEqual(result.positionals, ['input.js']);
assert.strictEqual(parseArgs({ args: ['--no-verbose'], options: { verbose: { type: 'boolean' } } }).values.verbose, false);
