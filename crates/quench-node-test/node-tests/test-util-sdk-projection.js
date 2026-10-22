'use strict';
const assert = require('assert');
const util = require('util');
const { internalBinding } = require('internal/test/binding');
(async () => {
  const rows = [];
  const names = ['MIMEType','MIMEParams','debug','diff','debounce','throttle','isArray',
    'isPartialDeepStrictEqual','markPromiseAsHandled','toUSVString','convertProcessSignalToExitCode',
    'getSystemErrorMap','getSystemErrorMessage','_errnoException','_exceptionWithHostPort'];
  for (const name of names) {
    const descriptor = Object.getOwnPropertyDescriptor(util, name);
    const value = util[name];
    rows.push([name, typeof value, value.name, value.length, descriptor.enumerable,
      descriptor.configurable, descriptor.writable, descriptor.get?.name, descriptor.set?.name]);
  }
  assert.strictEqual(util.debug, util.debuglog);
  assert.strictEqual(util.TextEncoder, TextEncoder);
  assert.strictEqual(util.TextDecoder, TextDecoder);
  assert.strictEqual(Object.getPrototypeOf(util), Object.prototype);
  const lazyTarget = {};
  internalBinding('util').defineLazyProperties(lazyTarget, 'internal/mime', ['MIMEType','MIMEParams'], false);
  assert.deepStrictEqual(Object.keys(lazyTarget), []);
  lazyTarget.MIMEType = 42;
  assert.strictEqual(lazyTarget.MIMEType, 42);
  assert.strictEqual(Object.create(lazyTarget).MIMEParams, util.MIMEParams);
  rows.push([Object.getOwnPropertyDescriptor(lazyTarget,'MIMEType'),
    Object.getOwnPropertyDescriptor(lazyTarget,'MIMEParams').enumerable,
    Object.getOwnPropertyDescriptor(lazyTarget,'MIMEParams').writable]);
  const mime = new util.MIMEType('Text/HTML;Charset="UTF-8";Foo=bar');
  rows.push([mime.type,mime.subtype,mime.essence,String(mime),Array.from(mime.params)]);
  mime.params.set('x','a b');mime.params.delete('foo');
  rows.push([String(mime),mime.params instanceof util.MIMEParams]);
  const params = new util.MIMEParams();params.set('a','b');rows.push([String(params),Array.from(params)]);
  rows.push([util.toUSVString('a\ud800b\udc00'),util.toUSVString({toString(){return 'ok\ud800'}})]);
  rows.push([util.diff('abc','adc'),util.diff(['a','b'],['a','c'])]);
  rows.push([util.isPartialDeepStrictEqual({a:1,b:2},{a:1}),
    util.isPartialDeepStrictEqual({a:1},{a:1,b:2}),util.isPartialDeepStrictEqual([1,2,3],[1,3])]);
  const a={};a.self=a;const b={};b.self=b;rows.push(util.isPartialDeepStrictEqual(a,b));
  const uv = internalBinding('uv');
  rows.push([Object.keys(uv),Object.getOwnPropertyDescriptor(uv,'UV_ENOENT'),
    uv.errname.name,uv.errname.length,uv.getErrorMap.name,uv.getErrorMap.length,
    uv.getErrorMessage.name,uv.getErrorMessage.length]);
  const map = util.getSystemErrorMap();
  rows.push(Array.from(map));
  const other = util.getSystemErrorMap();
  assert.notStrictEqual(map,other);assert.notStrictEqual(map.get(-2),other.get(-2));
  map.set(-2,['poison','poison']);
  rows.push([-1,-2,-7,-13,-4094,-4095,-3008,-99999].map(n =>
    [n,util.getSystemErrorName(n),util.getSystemErrorMessage(n)]));
  for(const value of [undefined,null,1,'1',{},Symbol('x'),Promise.prototype]) {
    try {util.markPromiseAsHandled(value);assert.fail('must reject non-Promise');}
    catch(e){rows.push([e.name,e.code,e.message]);}
  }
  const unhandled=[];process.on('unhandledRejection',e=>unhandled.push(e));
  const rejected=Promise.reject('handled');
  assert.strictEqual(util.markPromiseAsHandled(rejected),undefined);
  const debouncedCalls=[];
  const debounced=util.debounce(value=>{debouncedCalls.push(value);return value*2},1);
  const first=debounced(1),second=debounced(2);
  rows.push([await first,await second,debouncedCalls]);
  const throttled=util.throttle(value=>value+1,1,1);
  rows.push([await throttled(2),await throttled(3)]);
  const originalSet=Map.prototype.set, originalIterator=Array.prototype[Symbol.iterator];
  try {
    Map.prototype.set=()=>{throw new Error('mutable map setter invoked')};
    Array.prototype[Symbol.iterator]=()=>{throw new Error('mutable iterator invoked')};
    assert.strictEqual(util.getSystemErrorMap().size,other.size);
  } finally {Map.prototype.set=originalSet;Array.prototype[Symbol.iterator]=originalIterator;}
  rows.push(['signals',util.convertProcessSignalToExitCode('SIGTERM'),util.convertProcessSignalToExitCode('SIGINT')]);
  await new Promise(resolve=>setImmediate(resolve));assert.deepStrictEqual(unhandled,[]);
  console.log(JSON.stringify(rows));
})().catch(e=>{console.error(e);process.exitCode=1});
