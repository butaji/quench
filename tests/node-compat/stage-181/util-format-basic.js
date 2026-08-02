const util = require('util');

if (util.format([0]) !== '[ 0 ]') throw new Error('array format mismatch');
if (util.format({ foo: 42 }) !== '{ foo: 42 }') throw new Error('object format mismatch');
if (util.format('%i %f', '42.5', '1.5') !== '42 1.5') throw new Error('numeric format mismatch');
if (util.format('%d', 42.0) !== '42') throw new Error(`d format mismatch: ${util.format('%d', 42.0)}`);
if (util.format('foo', 'bar', 'baz') !== 'foo bar baz') throw new Error('extra argument format mismatch');
