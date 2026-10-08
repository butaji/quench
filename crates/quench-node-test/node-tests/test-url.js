// Node compat: url module.
const url = require('node:url');
if (url.parse.name !== 'urlParse' || url.parse.length !== 3 ||
  url.format.name !== 'urlFormat' || url.format.length !== 2) {
  throw new Error('URL function shape');
}
if (url.domainToASCII.name !== 'domainToASCII' || url.domainToASCII.length !== 1 ||
  url.domainToUnicode.name !== 'domainToUnicode' || url.domainToUnicode.length !== 1 ||
  url.domainToASCII('mañana.com') !== 'xn--maana-pta.com' ||
  url.domainToUnicode('xn--maana-pta.com') !== 'mañana.com' ||
  url.domainToUnicode('mañana.com') !== 'mañana.com' || url.domainToASCII(1) !== '0.0.0.1' ||
  url.domainToASCII('bad domain') !== '' || url.domainToUnicode('bad domain') !== '') {
  throw new Error('domain IDNA helpers');
}
const parsed = url.parse('http://x.example/y?z=1');
if (!(parsed.query === 'z=1')) throw new Error('query=' + parsed.query);
const formatted = url.format({ protocol: 'http:', hostname: 'h', pathname: '/p' });
if (formatted !== 'http://h/p') throw new Error('format=' + formatted);
if (url.format('http://x.example/y?z=1') !== 'http://x.example/y?z=1') {
  throw new Error('format string input');
}
if (url.format({ protocol: 'http:', hostname: 'h', pathname: '/p', query: { x: 'a b' } }) !==
  'http://h/p?x=a%20b') {
  throw new Error('format query object');
}
const whatwg = new url.URL('https://user:pass@example.com/p?q=1#frag');
if (url.format(whatwg, { fragment: false, search: false, auth: false }) !== 'https://example.com/p') {
  throw new Error('format WHATWG URL options');
}
if (url.format(whatwg, false) !== whatwg.href) throw new Error('format falsy options');
if (url.format({ protocol: 'https:', auth: 'u:p a', hostname: 'x', pathname: '?x#y' }) !==
  'https://u:p%20a@x/%3Fx%23y') {
  throw new Error('format auth and path escaping');
}
console.log('url: %s', parsed.query);
