'use strict';
const out = [];
const api = {inner: {value: 'inner', read(value) { return value.value; }},
  forward(value) { return this.inner.read(value); },
  changed(value) { value = this.inner; return value.read(value); }};
for (let i = 0; i < 80; i++) {
  if (api.forward({value: i}) !== i || api.changed({value: 'wrong'}) !== 'inner') throw new Error('forwarded parameters');
}
out.push(api.forward({value: 'forward'}), api.changed({value: 'wrong'}));
api.inner.read = value => value.other;
out.push(api.forward({other: 'replacement'}));
console.log(JSON.stringify(out));

{
const api = {
  inner: {
    value: 'inner', values: ['inner-0', 'inner-1'],
    read(value) { return value.value; },
    at(value, index) { return value.values[index]; },
    none() { return this.value; }
  },
  forward(value) { return this.inner.read(value); },
  alias(value) { value = this.inner; return value.read(value); },
  two(value, index) { return this.inner.at(value, index); },
  aliasTwo(value, index) { value = this.inner; return value.at(value, index); },
  zero() { return this.inner.none(); },
  local(value) { let receiver = this.inner; return receiver.read(value); }
};
const results = [];
for (let i = 0; i < 12; i++) {
  results.push([api.forward({value: i}), api.alias({value: 'wrong'}),
    api.two({values: [i, i + 1]}, 1), api.aliasTwo({values: ['wrong', 'wrong']}, 1),
    api.zero(), api.local({value: 'local'})]);
}
console.log(JSON.stringify(results));
api.inner.read = value => value.other;
console.log(api.forward({other: 'replacement'}));
const effects = [];
Object.defineProperty(api.inner, 'read', {configurable: true, get() {
  effects.push('get'); return value => value.value;
}});
console.log(api.forward({value: 'getter'}));
const ordinary = {read(value) { return value.value; }};
api.inner = new Proxy(ordinary, {get(target, key, receiver) {
  effects.push('proxy:' + key); return Reflect.get(target, key, receiver);
}});
console.log(api.forward({value: 'proxy'}));
api.withFinally = function (value) {
  try { return this.inner.read(value); }
  finally { effects.push('finally'); }
};
console.log(api.withFinally({value: 'handled'}));
const marker = {};
ordinary.read = () => { throw marker; };
api.withCatch = function (value) {
  try { return this.inner.read(value); }
  catch (error) { return error === marker ? 'caught' : 'wrong'; }
};
console.log(api.withCatch({value: 'ignored'}));
console.log(JSON.stringify(effects));

}
