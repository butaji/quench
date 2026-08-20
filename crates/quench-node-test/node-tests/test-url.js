// Node compat: url module.
const url = require('node:url');
const parsed = url.parse('http://x.example/y?z=1');
if (!(parsed.query === 'z=1')) throw new Error('query=' + parsed.query);
const formatted = url.format({ protocol: 'http:', hostname: 'h', pathname: '/p' });
if (!(formatted.indexOf('http://h/p') === 0)) throw new Error('format=' + formatted);
console.log('url: %s', parsed.query);
