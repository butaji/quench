'use strict';
// console.log of primitives: `formatWithOptions` writes strings as they are and every other
// primitive as `inspect` would with the console's options; a first string with `%` formats.
// `FORCE_COLOR` colorizes output that is not a terminal, and `inspect.defaultOptions`
// apply once `util` is loaded.
console.log(1);
console.log(-0, 0, 1.5, 1e21, 1e-7, NaN, -Infinity);
console.log(10n, -3n, true, false, null, undefined);
console.log('a', 1, '%d', 2);
console.log(1, '%s');
console.log('x %d %s', 5, 6n);
console.log();
console.log('');
process.env.FORCE_COLOR = '1';
console.log(7, false);
delete process.env.FORCE_COLOR;
console.log(8, null);
require('util').inspect.defaultOptions.numericSeparator = true;
console.log(1000000, 2n ** 70n, 'plain');
