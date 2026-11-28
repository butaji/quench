function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (y)['c']); for (let j = 0; j < 6; j++) s = (s + (x%5)); o.a = x; for (let j = 0; j < 2; j++) s = (s + (new P(Math.round(i)).v - ((7 == (x%5))?(x <= x):k))); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + 0.5); try { s = (s + String((arr)['length'])); } catch (e) { s = s + e.name; } return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (cap = (cap + i) | 0)); s = (s + (x)['a']); s = (s + y); return s; }
function f3(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; o.a = ((x*y) != ((0.5 | (x*y))?(k ?? i):((x*y)?i:(x%5)))); s = (s + (((7 != (x+y)) | y) ?? new P((x%5)).v)); o.a = (((y % (x|0)) + ((x%5) ^ 0.5)) <= Math.trunc(3)); return s; }
const A = [undefined,-0,1,NaN,[1,2,3]]; const B = [new Q(1),undefined,new P(3),{a:'s',b:null},1]; const C = ['bc',new P(3),null,1.5,1.5]; const A2 = [{a:1,b:2,c:3},{a:1,b:2},{a:1,b:2},{a:1,b:2},NaN]; const B2 = [{a:1,b:2},2,{a:'s',b:null},-0,{a:1,b:2}]; const C2 = [0,'bc','',null,undefined];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f3(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
