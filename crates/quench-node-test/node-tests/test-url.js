// Node compat: url module.
const url = require('node:url');
if (typeof url.URLSearchParams !== 'function' || url.URLSearchParams !== URLSearchParams) {
  throw new Error('URLSearchParams export/global identity');
}
const params = new url.URLSearchParams('a=one+two&a=three&b=%21');
if (params.get('a') !== 'one two' || params.getAll('a').join('|') !== 'one two|three' ||
  params.get('missing') !== null || params.size !== 3 || params.toString() !== 'a=one+two&a=three&b=%21') {
  throw new Error('URLSearchParams query parsing');
}
const liveUrl = new URL('https://example.test/path?old=1#frag');
const liveParams = liveUrl.searchParams;
liveParams.append('new value', '!');
if (liveUrl.href !== 'https://example.test/path?old=1&new+value=%21#frag' ||
  liveUrl.searchParams !== liveParams) throw new Error('URL.searchParams mutation and identity');
liveUrl.search = '?replacement=2';
if (liveUrl.searchParams !== liveParams || liveParams.get('replacement') !== '2' ||
  liveUrl.href !== 'https://example.test/path?replacement=2#frag') {
  throw new Error('URL.search setter updates live searchParams');
}
params.set('a', 'new value');
params.append('x y', '!~');
if (params.toString() !== 'a=new+value&b=%21&x+y=%21%7E' || !params.has('a', 'new value')) {
  throw new Error('URLSearchParams mutation and encoding');
}
params.sort();
if ([...params.keys()].join(',') !== 'a,b,x y') throw new Error('URLSearchParams sort/iterator');
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
if (url.resolve('http://x.example/a/b', '../c?x=1') !== 'http://x.example/c?x=1') {
  throw new Error('legacy URL resolve');
}
const resolvedObject = url.resolveObject('http://x.example/a/b', '../c?x=1');
if (resolvedObject.hostname !== 'x.example' || resolvedObject.pathname !== '/c' ||
  resolvedObject.query !== 'x=1') {
  throw new Error('legacy URL resolveObject');
}
const pathBuffer = url.fileURLToPathBuffer(new url.URL('file:///tmp/hello%20world'));
if (!Buffer.isBuffer(pathBuffer) || pathBuffer.toString() !== '/tmp/hello world') {
  throw new Error('fileURLToPathBuffer conversion');
}
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
const idnUrl = new url.URL('https://xn--maana-pta.com/');
if (url.format(idnUrl, { unicode: true }) !== 'https://mañana.com/') {
  throw new Error('format WHATWG URL Unicode option');
}
if (url.format({ protocol: 'https:', auth: 'u:p a', hostname: 'x', pathname: '?x#y' }) !==
  'https://u:p%20a@x/%3Fx%23y') {
  throw new Error('format auth and path escaping');
}
if (url.urlToHttpOptions.name !== 'urlToHttpOptions' || url.urlToHttpOptions.length !== 1) {
  throw new Error('urlToHttpOptions function shape');
}
const httpOptions = url.urlToHttpOptions(new url.URL('https://user:pass@example.com:444/a?b#c'));
if (Object.getPrototypeOf(httpOptions) !== null || httpOptions.protocol !== 'https:' ||
  httpOptions.hostname !== 'example.com' || httpOptions.pathname !== '/a' ||
  httpOptions.search !== '?b' || httpOptions.hash !== '#c' || httpOptions.path !== '/a?b' ||
  httpOptions.href !== 'https://user:pass@example.com:444/a?b#c' || httpOptions.port !== 444 ||
  httpOptions.auth !== 'user:pass') {
  throw new Error('urlToHttpOptions fields');
}
const emptyHttpOptions = url.urlToHttpOptions({});
if (Object.getPrototypeOf(emptyHttpOptions) !== null || emptyHttpOptions.path !== '' ||
  !Number.isNaN(emptyHttpOptions.port) || emptyHttpOptions.href !== undefined) {
  throw new Error('urlToHttpOptions plain object handling');
}
console.log('url: %s', parsed.query);
