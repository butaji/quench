// Node compat: url module.
const url = require('node:url');
const parsed = url.parse('http://x.example/y?z=1');
if (!(parsed.protocol === 'http:')) throw new Error('protocol=' + parsed.protocol);
if (!(parsed.host === 'x.example')) throw new Error('host=' + parsed.host);
if (!(parsed.pathname === '/y')) throw new Error('pathname=' + parsed.pathname);
if (!(parsed.path === '/y?z=1')) throw new Error('path=' + parsed.path);
if (!(parsed.query === 'z=1')) throw new Error('query=' + parsed.query);
const formatted = url.format({ protocol: 'http:', hostname: 'h', pathname: '/p' });
if (!(formatted.indexOf('http://h/p') === 0)) throw new Error('format=' + formatted);
console.log('url: %s', parsed.query);
