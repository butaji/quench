'use strict';
// console's writing methods with primitives: `formatWithOptions` writes strings as they are
// and every other primitive as `inspect` would with the console's options; a first string
// with `%` formats when more arguments follow. `FORCE_COLOR` colorizes output that is not a
// terminal, `inspect.defaultOptions` apply once `util` is loaded, and a `console.*` channel
// subscriber sees every call once `diagnostics_channel` is loaded.
const errors = [];
process.stderr.write = (chunk) => { errors.push(String(chunk)); return true; };
console.log(1);
console.log(-0, 0, 1.5, 1e21, 1e-7, NaN, -Infinity);
console.log(10n, -3n, true, false, null, undefined);
console.log('a', 1, '%d', 2);
console.log(1, '%s');
console.log('x %d %s', 5, 6n);
console.log('lone %d');
console.log();
console.log('');
console.info('info', 1);
console.debug('debug', false);
console.warn('warn %s', 2);
console.error('error', null);
console.error('lone %s');
process.env.FORCE_COLOR = '1';
console.log(7, false);
console.error(7);
delete process.env.FORCE_COLOR;
console.log(8, null);
const channel = require('diagnostics_channel').channel('console.log');
const seen = [];
channel.subscribe((args) => seen.push(args.length));
console.log('observed', 9);
require('util').inspect.defaultOptions.numericSeparator = true;
console.log(1000000, 2n ** 70n, 'plain');
console.log(JSON.stringify({ errors, seen }));
