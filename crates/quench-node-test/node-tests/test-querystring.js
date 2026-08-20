// Node compat: querystring module.
const querystring = require('node:querystring');
const parsed = querystring.parse('a=1&b=2&a=3');
if (!(parsed.a[0] === '1')) throw new Error('a0=' + parsed.a[0]);
if (!(parsed.a[1] === '3')) throw new Error('a1=' + parsed.a[1]);
if (!(parsed.b[0] === '2')) throw new Error('b=' + parsed.b[0]);
const encoded = querystring.stringify({ foo: 'bar', baz: ['qux', 'quux'] });
if (!(encoded === 'foo=bar&baz=qux&baz=quux')) throw new Error('enc=' + encoded);
const escaped = querystring.escape('hello world&foo=bar');
if (escaped.indexOf('hello%20world') < 0) throw new Error('escape=' + escaped);
const unescaped = querystring.unescape('hello%20world%26foo%3Dbar');
if (!(unescaped === 'hello world&foo=bar')) throw new Error('unescape=' + unescaped);
console.log('querystring: ' + encoded);
