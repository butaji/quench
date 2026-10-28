'use strict';
const out = [];
function readLength(x) { return x.length; }
function readSize(x) { return x.size; }
function readByteLength(x) { return x.byteLength; }
const P = { length: 'proto-length', size: 'proto-size', byteLength: 'proto-bl' };
const Q = Object.create(P);
const deep = Object.create(Q);
const arr = [1, 2, 3];
Object.setPrototypeOf(arr, P);
const str = new String('abcd');
Object.setPrototypeOf(str, P);
function fn(a, b) {}
Object.setPrototypeOf(fn, P);
const map = new Map([[1, 1]]);
const mapP = new Map([[1, 1], [2, 2]]);
Object.setPrototypeOf(mapP, P);
const ta = new Uint8Array(5);
const taP = new Uint8Array(6);
Object.setPrototypeOf(taP, P);
const args = (function () { return arguments; })(1, 2);
const receivers = [
  ['plain', Object.create(P)], ['deep', deep], ['own', { length: 7, size: 8, byteLength: 9 }],
  ['arr', arr], ['arr2', [5, 6]], ['str', str], ['fn', fn], ['map', map], ['mapP', mapP],
  ['ta', ta], ['taP', taP], ['args', args], ['plain2', Object.create(P)], ['deep2', Object.create(Object.create(Q))],
];
for (let round = 0; round < 4; round++) {
  for (const [name, r] of receivers) {
    out.push(`${round} ${name} ${String(readLength(r))} ${String(readSize(r))} ${String(readByteLength(r))}`);
  }
}
// Shape-sharing: an empty ordinary object and an empty array share the root transition.
const A = { length: 'A-len' };
const o1 = Object.create(A);
const a1 = []; Object.setPrototypeOf(a1, A);
for (let i = 0; i < 6; i++) out.push(`share ${readLength(i % 2 ? a1 : o1)}`);
// Mutating the prototype after caching.
const M = { size: 1 };
const m1 = Object.create(M);
for (let i = 0; i < 3; i++) out.push(`mut ${readSize(m1)}`);
M.size = 2; out.push(`mut ${readSize(m1)}`);
Object.defineProperty(M, 'size', { get() { return 'getter'; } }); out.push(`mut ${readSize(m1)}`);
console.log(out.join('\n'));
