function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; if ((x|0)) s = (s + k); else s = (s + (x+y)); o.a = (Math.trunc(((x|0)?y:i))?(0.5 & (7 | (x+y))):(i + y)); s = (s + (cap = (cap + new P(y).v) | 0)); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + ((o)['1'] - (7 << y))); if (new P((((y>>>1) ?? y) * (0.5?(y>>>1):k))).v) s = (s + ((0.5?3:new P((x%5)).v) < (((x*y) <= k) == String((x*y))))); else s = (s + x); o.a = new P((y>>>1)).v; o.c = 0.5; return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; for (let j = 0; j < 3; j++) s = (s + 3); s = (s + (String(String((x+y))) || (x)['length'])); s = (s + ((7 <= (o)['c']) << new P(k).v)); return s; }
function f3(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; try { s = (s + i); } catch (e) { s = s + e.name; } s = (s + new P(7).v); return s; }
const A = [2,new Q(1),'',NaN,0]; const B = [[1.5,2,'x'],[1,2,3],{b:2,a:1},undefined,7]; const C = [undefined,[1.5,2,'x'],4294967296,-2147483648,1]; const A2 = ['',0.25,true,'bc',{a:1,b:2,c:3}]; const B2 = [new P(3),'a',{b:2,a:1},-2147483648,1.5]; const C2 = [7,4294967296,{a:1,b:2,c:3},1,undefined];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f3(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
