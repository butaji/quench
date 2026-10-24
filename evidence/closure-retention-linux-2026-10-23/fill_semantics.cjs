// Array.prototype.fill edge cases; prints one line per case for diffing against Node.
const out = [];
const show = (name, f) => {
  try { out.push(name + ' ' + JSON.stringify(f())); } catch (e) { out.push(name + ' throws ' + e.constructor.name); }
};
show('big holey', () => { const a = new Array(1_000_000).fill('data'); return [a.length, a[0], a[999_999], 999_999 in a]; });
show('partial range', () => [1, 2, 3, 4, 5].fill(0, 1, -1));
show('negative start', () => [1, 2, 3].fill(9, -2));
show('empty range', () => [1, 2, 3].fill(9, 2, 1));
show('holey dense', () => { const a = [1, , 3, , 5]; a.fill(7, 1, 4); return [a, Object.keys(a)]; });
show('sparse small range', () => { const a = new Array(1_000_000); a.fill(1, 10, 12); return [a.length, Object.keys(a), a[11]]; });
show('proto setter', () => {
  const log = [];
  Object.defineProperty(Array.prototype, 3, { set(v) { log.push(v); }, configurable: true });
  try { const a = new Array(5); a.fill('x'); return [log, Object.keys(a)]; } finally { delete Array.prototype[3]; }
});
show('proto readonly', () => {
  Object.defineProperty(Object.prototype, 1, { value: 'ro', writable: false, configurable: true });
  try { const a = new Array(3); a.fill('x'); return Object.keys(a); } finally { delete Object.prototype[1]; }
});
show('frozen', () => Object.freeze([1, 2]).fill(0));
show('sealed holey', () => { const a = Object.seal([1, , 3]); a.fill(0); return a; });
show('sealed full', () => { const a = Object.seal([1, 2, 3]); a.fill(0); return a; });
show('nonextensible holey', () => { const a = Object.preventExtensions([1, , 3]); a.fill(0); return a; });
show('readonly element', () => { const a = [1, 2, 3]; Object.defineProperty(a, 1, { writable: false }); a.fill(0); return a; });
show('accessor element', () => { const log = []; const a = [1, 2, 3]; Object.defineProperty(a, 1, { set(v) { log.push(v); } }); a.fill(5); return [log, a[0], a[2]]; });
show('subclass', () => { class X extends Array {} const x = new X(3).fill(4); return [x instanceof X, x.slice()]; });
show('subclass proto index', () => { class X extends Array {} X.prototype[0] = 'p'; const x = new X(2); x.fill(1); return [x[0], Object.keys(x)]; });
show('arguments', () => (function () { Array.prototype.fill.call(arguments, 8); return [...arguments]; })(1, 2));
show('array-like', () => Array.prototype.fill.call({ length: 2 }, 3));
show('valueOf mutates', () => { const a = [1, 2, 3]; a.fill(0, { valueOf() { a.length = 1; return 0; } }); return [a.length, a]; });
show('shared literal', () => { const f = () => [1, 2, 3]; const a = f(); a.fill(0); return [a, f()]; });
show('object values', () => { const o = {}; const a = new Array(3).fill(o); return [a[0] === a[2], a.length]; });
console.log(out.join('\n'));
