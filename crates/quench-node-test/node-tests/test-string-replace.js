// string — String.prototype.replace must work on strings carrying
// non-ASCII/code-unit content (e.g. ANSI escapes from source literals),
// not return an empty string. Regression for a replaced StringUnits bug.
'use strict';
const assert = require('assert');

const colored = '\u001b[31mred\u001b[39m';

// Remove just the leading escape; the rest of the string must survive.
assert.strictEqual(colored.replace(/\u001b/, ''), '[31mred\u001b[39m', 'leading ESC');
// Replace a digit only, keeping surrounding text.
assert.strictEqual(colored.replace(/[0-9]/, 'X'), '\u001b[X1mred\u001b[39m', 'class replace');
// A string pattern on code-unit text.
assert.strictEqual('\u001babc'.replace('b', 'X'), '\u001baXc', 'string pattern');
// replaceAll shares the fixed code-unit path.
assert.strictEqual('\u001baba'.replaceAll('a', 'X'), '\u001bXbX', 'replaceAll');

console.log('string-replace: ok');