var N = 3; let L = 'let'; const C = 'const'; function F() { return 'fn'; }
const out = [];
function read() { return [N, L, C, F(), typeof globalThis.N, typeof globalThis.F].join(','); }
out.push(read()); N = 4; L = 'let2'; out.push(read());
function tdz() { try { return T; } catch (e) { return e.constructor.name; } }
out.push(tdz()); let T = 't'; out.push(tdz());
out.push(eval('var E = 5; E'), typeof E, (function(){ 'use strict'; return eval('var S = 6; S') + typeof S; })());
function readE() { return E; } out.push(readE());
for (var i = 0; i < 3; i++) out.push((function(){ return i + N; })());
console.log(out.join(' '));
