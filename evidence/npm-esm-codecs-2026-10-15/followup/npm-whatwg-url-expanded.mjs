import assert from 'node:assert/strict';

// A finite public API corpus. Each check names only the callable it exercises;
// conditional entry points, symbols and arbitrary inputs remain obligations.
export function exercise(api) {
  let assertions = 0;
  const checks = {};
  const equal = (id, actual, expected) => {
    assert.deepEqual(actual, expected); assertions++;
    checks[id] = (checks[id] ?? 0) + 1;
  };
  const throws = (id, call, error) => {
    assert.throws(call, error); assertions++;
    checks[id] = (checks[id] ?? 0) + 1;
  };
  const base = api.parseURL('https://example.com/a/');
  for (const [input, expected] of [
    ['../b?q=1#f', 'https://example.com/b?q=1#f'],
    ['https://EXAMPLE.com:443/a/../b', 'https://example.com/b'],
    ['http://127.1/', 'http://127.0.0.1/'],
    ['https://[2001:db8::1]/', 'https://[2001:db8::1]/'],
    ['file:///C|/a/../b', 'file:///C:/b'],
    ['mailto:a@example.com', 'mailto:a@example.com'],
    ['https://例え.テスト/é', 'https://xn--r8jz45g.xn--zckzah/%C3%A9'],
  ]) {
    const record = api.parseURL(input, {baseURL: base});
    equal('parseURL', api.serializeURL(record), expected);
    equal('serializeURL', api.serializeURL(record), expected);
    equal('basicURLParse', api.basicURLParse(input, {baseURL: base}), record);
    const url = new api.URL(input, 'https://example.com/a/');
    equal('URL', url.href, expected);
    equal('URL.prototype.toString', url.toString(), expected);
    equal('URL.prototype.toJSON', url.toJSON(), expected);
    equal('URL.parse', api.URL.parse(input, 'https://example.com/a/').href, expected);
    equal('URL.canParse', api.URL.canParse(input, 'https://example.com/a/'), true);
  }
  equal('parseURL', api.parseURL('https://ex ample.test/'), null);
  equal('URL.parse', api.URL.parse('https://ex ample.test/'), null);
  equal('URL.canParse', api.URL.canParse('https://ex ample.test/'), false);
  throws('URL', () => new api.URL('https://ex ample.test/'), TypeError);
  throws('URL.prototype.toString', () => api.URL.prototype.toString.call({}), TypeError);
  for (const [input, errors, valid] of [
    ['http://example.com', [], true],
    ['http:example.com', ['special-scheme-missing-following-solidus'], false],
    ['https://example.com/%GG', ['invalid-URL-unit'], false],
    ['https://ex ample.test/', ['domain-to-ASCII'], false],
    ['relative', ['missing-scheme-non-relative-URL'], false],
  ]) {
    const result = api.parseURLWithValidationErrors(input);
    equal('parseURLWithValidationErrors', result.validationErrors, errors);
    equal('parseURLWithValidationErrors', result.url, api.parseURL(input));
    equal('isValidURLString', api.isValidURLString(input), valid);
  }
  equal('isValidURLString', api.isValidURLString('../x', {baseURL: base}), true);
  const record = api.parseURL('https://example.com:8443/a/b?q=1#f');
  equal('serializePath', api.serializePath(record), '/a/b');
  equal('serializeURLOrigin', api.serializeURLOrigin(record), 'https://example.com:8443');
  equal('serializeURLOrigin', api.serializeURLOrigin(api.parseURL('mailto:x@y')), 'null');
  for (const input of ['http://127.1/', 'https://[2001:db8::1]/', 'https://example.com/']) {
    const parsed = api.parseURL(input);
    equal('serializeHost', api.serializeHost(parsed.host), new api.URL(input).hostname);
  }
  for (const number of [0, 42, 65535]) equal('serializeInteger', api.serializeInteger(number), String(number));
  equal('setTheUsername', api.setTheUsername(record, 'a b@'), undefined);
  equal('setThePassword', api.setThePassword(record, 'p:/'), undefined);
  equal('setTheUsername', record.username, 'a%20b%40');
  equal('setThePassword', record.password, 'p%3A%2F');
  equal('serializeURL', api.serializeURL(record, true), 'https://a%20b%40:p%3A%2F@example.com:8443/a/b?q=1');
  for (const [input, cannot, opaque] of [
    ['https://example.com/', false, false], ['file:///tmp/a', true, false], ['data:text/plain,x', true, true],
  ]) {
    const parsed = api.parseURL(input);
    equal('cannotHaveAUsernamePasswordPort', api.cannotHaveAUsernamePasswordPort(parsed), cannot);
    equal('hasAnOpaquePath', api.hasAnOpaquePath(parsed), opaque);
  }
  equal('percentDecodeString', Array.from(api.percentDecodeString('a%20%C3%A9%GG')), [97,32,195,169,37,71,71]);
  equal('percentDecodeBytes', Array.from(api.percentDecodeBytes(new Uint8Array([37,52,49,37,50,48,255]))), [65,32,255]);
  const url = new api.URL('https://u:p@example.com:8443/a?x=1#f');
  const params = url.searchParams;
  for (const [key, value] of Object.entries({protocol:'http:',username:'a b',password:'p@',hostname:'other.test',port:'8080',pathname:'/é',search:'?b=2',hash:'#h'})) url[key] = value;
  equal('URL', url.href, 'http://a%20b:p%40@other.test:8080/%C3%A9?b=2#h');
  equal('URL', url.origin, 'http://other.test:8080');
  equal('URL', url.searchParams, params);
  equal('URLSearchParams.prototype.get', params.get('b'), '2');
  url.host = 'next.test:9090';
  equal('URL', url.host, 'next.test:9090');
  url.href = 'https://reset.test/?z=3';
  equal('URLSearchParams.prototype.get', params.get('z'), '3');
  for (const [input, expected] of [['?a=1&a=2+b','a=1&a=2+b'], [{x:'é'},'x=%C3%A9'], [[['x','1'],['y','2']],'x=1&y=2']]) {
    equal('URLSearchParams', new api.URLSearchParams(input).toString(), expected);
  }
  const p = new api.URLSearchParams('b=2&a=1&a=3');
  equal('URLSearchParams.prototype.append', p.append('c','x y'), undefined);
  equal('URLSearchParams.prototype.get', p.get('a'), '1');
  equal('URLSearchParams.prototype.get', p.get('missing'), null);
  equal('URLSearchParams.prototype.getAll', p.getAll('a'), ['1','3']);
  equal('URLSearchParams.prototype.has', p.has('a','3'), true);
  equal('URLSearchParams.prototype.has', p.has('a','4'), false);
  equal('URLSearchParams.prototype.delete', p.delete('a','1'), undefined);
  equal('URLSearchParams.prototype.set', p.set('b','4'), undefined);
  equal('URLSearchParams.prototype.sort', p.sort(), undefined);
  equal('URLSearchParams.prototype.toString', p.toString(), 'a=3&b=4&c=x+y');
  equal('URLSearchParams.prototype.keys', Array.from(p.keys()), ['a','b','c']);
  equal('URLSearchParams.prototype.values', Array.from(p.values()), ['3','4','x y']);
  equal('URLSearchParams.prototype.entries', Array.from(p.entries()), [['a','3'],['b','4'],['c','x y']]);
  const visits = [], receiver = {};
  equal('URLSearchParams.prototype.forEach', p.forEach(function(value,key,object) {visits.push([key,value,this===receiver,object===p]);},receiver), undefined);
  equal('URLSearchParams.prototype.forEach', visits, [['a','3',true,true],['b','4',true,true],['c','x y',true,true]]);
  equal('URLSearchParams', p.size, 3);
  throws('URLSearchParams', () => new api.URLSearchParams([['one']]), TypeError);
  throws('URLSearchParams.prototype.get', () => api.URLSearchParams.prototype.get.call({},'x'), TypeError);
  const initial = 'https://user:pass@example.com:8443/a?x=1#f';
  for (const [key, expected] of Object.entries({
    href:initial, origin:'https://example.com:8443', protocol:'https:',
    username:'user', password:'pass', host:'example.com:8443', hostname:'example.com',
    port:'8443', pathname:'/a', search:'?x=1', hash:'#f'
  })) {
    const descriptor = Object.getOwnPropertyDescriptor(api.URL.prototype,key);
    equal(`URL.prototype.${key}:get`, descriptor.get.call(new api.URL(initial)), expected);
    throws(`URL.prototype.${key}:get`, () => descriptor.get.call({}), TypeError);
  }
  for (const [key, input, expected] of [
    ['href','http://other.test/b?y=2#h','http://other.test/b?y=2#h'],
    ['protocol','http:','http:'], ['username','a b@','a%20b%40'],
    ['password','p:/','p%3A%2F'], ['host','other.test:9090','other.test:9090'],
    ['hostname','other.test','other.test'], ['port','443',''],
    ['pathname','/é x','/%C3%A9%20x'], ['search','?é=x y','?%C3%A9=x%20y'],
    ['hash','#é x','#%C3%A9%20x']
  ]) {
    const object = new api.URL(initial);
    const descriptor = Object.getOwnPropertyDescriptor(api.URL.prototype,key);
    equal(`URL.prototype.${key}:set`, descriptor.set.call(object,input), undefined);
    equal(`URL.prototype.${key}:get`, descriptor.get.call(object), expected);
    throws(`URL.prototype.${key}:set`, () => descriptor.set.call({},input), TypeError);
  }
  const href = Object.getOwnPropertyDescriptor(api.URL.prototype,'href');
  throws('URL.prototype.href:set', () => href.set.call(new api.URL(initial),'http://['), TypeError);
  const invalidPort = new api.URL(initial);
  invalidPort.port = '65536';
  equal('URL.prototype.port:set', invalidPort.port, '8443');
  invalidPort.protocol = 'mailto:';
  equal('URL.prototype.protocol:set', invalidPort.protocol, 'https:');
  const searchParams = Object.getOwnPropertyDescriptor(api.URL.prototype,'searchParams');
  const object = new api.URL(initial), instance = searchParams.get.call(object);
  equal('URL.prototype.searchParams:get', searchParams.get.call(object), instance);
  equal('URL.prototype.searchParams:get', instance.toString(), 'x=1');
  object.search = '?y=2';
  equal('URL.prototype.searchParams:get', instance.toString(), 'y=2');
  throws('URL.prototype.searchParams:get', () => searchParams.get.call({}), TypeError);
  const size = Object.getOwnPropertyDescriptor(api.URLSearchParams.prototype,'size');
  equal('URLSearchParams.prototype.size:get', size.get.call(new api.URLSearchParams('a=1&a=2&b=3')), 3);
  equal('URLSearchParams.prototype.size:get', size.get.call(new api.URLSearchParams()), 0);
  throws('URLSearchParams.prototype.size:get', () => size.get.call({}), TypeError);
  const iterator = api.URLSearchParams.prototype[Symbol.iterator];
  equal('URLSearchParams.prototype.[Symbol.iterator]', Array.from(iterator.call(new api.URLSearchParams('a=1&a=2'))), [['a','1'],['a','2']]);
  throws('URLSearchParams.prototype.[Symbol.iterator]', () => iterator.call({}), TypeError);
  return {package:'whatwg-url',assertions,checks,scope:'Finite public parser, serializer, URL and search-parameter corpus; matching cases do not prove all inputs or conditional entries'};
}
