const util = require('util');

const previous = util.inspect.defaultOptions.numericSeparator;
util.inspect.defaultOptions.numericSeparator = true;
if (util.format('%d %s %i', 123456789, 123456789, 123456789) !== '123_456_789 123_456_789 123_456_789') throw new Error('numeric separator mismatch');
util.inspect.defaultOptions.numericSeparator = previous;
if (util.formatWithOptions({ numericSeparator: true }, '%d', 123456789) !== '123_456_789') throw new Error('formatWithOptions separator mismatch');
