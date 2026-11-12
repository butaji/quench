const out = [];
const L = (s) => out.push(String(s));
const tick = (n) => { let p = Promise.resolve(); for (let i = 0; i < n; i++) p = p.then(() => L('tick' + i)); return p; };
process.on('unhandledRejection', (r) => L('unhandled ' + (r && r.message || r)));

// plain combinators with mixed inputs
const p1 = Promise.resolve(1);
Promise.all([p1, 2, Promise.resolve(3), { then(r) { L('thenable.then'); r(4); } }]).then((v) => L('all ' + v));
Promise.race([new Promise(() => {}), p1, 5]).then((v) => L('race ' + v));
Promise.allSettled([p1, Promise.reject(new Error('x')), 7]).then((v) => L('allSettled ' + JSON.stringify(v.map((r) => r.status + ':' + (r.value ?? r.reason.message)))));
Promise.any([Promise.reject(1), Promise.reject(2), p1]).then((v) => L('any ' + v));
Promise.any([Promise.reject(1), Promise.reject(2)]).catch((e) => L('any-rej ' + e.constructor.name + ' ' + e.errors));
Promise.all([Promise.reject(new Error('r1')), p1]).catch((e) => L('all-rej ' + e.message));
Promise.all([]).then((v) => L('all-empty ' + v.length));
Promise.race([]).then(() => L('race-empty-never'));
Promise.all('ab').then((v) => L('all-string ' + v));
Promise.all(new Set([1, p1])).then((v) => L('all-set ' + v));

// withResolvers
const wr = Promise.withResolvers();
wr.promise.then((v) => L('wr ' + v));
wr.resolve(9); wr.resolve(10);
const wr2 = Promise.withResolvers();
wr2.promise.catch((v) => L('wr2 ' + v));
wr2.reject('no'); wr2.resolve('late');

// patched Promise.resolve is observed per element
const origResolve = Promise.resolve;
let calls = 0;
Promise.resolve = function (x) { calls++; return origResolve.call(this, x); };
Promise.all([1, 2, 3]).then(() => { L('patched resolve calls ' + calls); Promise.resolve = origResolve; });

// patched then on an element promise is observed
const q = Promise.resolve('q');
let thenCalls = 0;
const origThen = q.then;
q.then = function (...a) { thenCalls++; return origThen.apply(this, a); };
Promise.all([q]).then((v) => L('patched then calls ' + thenCalls + ' ' + v));

// subclass: species/constructor path
class MyP extends Promise { static get [Symbol.species]() { return Promise; } }
MyP.all([1, 2]).then((v) => L('MyP.all ' + v + ' ' + (MyP.all([]) instanceof MyP)));
class Sub extends Promise { constructor(ex) { L('Sub ctor'); super(ex); } }
Sub.all([1]).then((v) => L('Sub.all ' + v));
Sub.withResolvers().resolve(1);

// getters on constructor.resolve
const weird = function (ex) { return new Promise(ex); };
Object.defineProperty(weird, 'resolve', { get() { L('resolve getter'); return Promise.resolve.bind(Promise); } });
Promise.all.call(weird, [1]).then((v) => L('weird ' + v));

// Array.prototype.then poison: resolve(array) observes then
Object.defineProperty(Array.prototype, 'then', { configurable: true, get() { L('array then read'); return undefined; } });
Promise.all([1]).then((v) => { delete Array.prototype.then; L('poison ' + v); });

// new Promise(executor) behaviour
new Promise((res, rej) => { L('exec'); res(1); rej(2); }).then((v) => L('np ' + v));
new Promise(() => { throw new Error('exec-throw'); }).catch((e) => L('np-throw ' + e.message));

tick(12).then(() => setTimeout(() => console.log(out.join('\n')), 5));
