const qs = require('node:querystring');

const parsed = qs.parse('a=1;b=2;c=3', ';', '=', { maxKeys: 2 });
if (parsed.a !== '1' || parsed.b !== '2' || parsed.c !== undefined) throw new Error('maxKeys/separator');
const repeated = qs.parse('x=1&x=2&x=3');
if (!Array.isArray(repeated.x) || repeated.x.join(',') !== '1,2,3') throw new Error('repeated values');
const encoded = qs.stringify({ a: null, b: undefined, c: false, d: 0, e: ['x', null] });
if (encoded !== 'a=&b=&c=false&d=0&e=x&e=') throw new Error('coercion: ' + encoded);
if (qs.unescape('%E0%A4%A') !== '�%A') throw new Error('malformed utf8 fallback');
console.log('querystring edge: ok');
