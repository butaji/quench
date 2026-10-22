import def, { counter, bump } from './dep.mjs';
const out = [];
function readCounter() { return counter; }
out.push(readCounter()); bump(); out.push(readCounter()); bump(); out.push(readCounter());
out.push(def());
function early() { try { return later; } catch (e) { return e.constructor.name; } }
out.push(early());
let later = 'init';
out.push(early());
var v = 1; function rv() { return v; } out.push(rv()); v = 2; out.push(rv());
out.push(typeof globalThis.v);
console.log(out.join(' '));
