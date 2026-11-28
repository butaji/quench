function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; if ((y>>>1)) s = (s + (x%5)); else s = (s + ((o)['length'] ^ (((x%5) & (x*y)) % (k + 7)))); if (((x+y)?(new P((x*y)).v?(0.5?(y>>>1):3):String((y>>>1))):((7 >> i) != (y>>>1)))) s = (s + (((x - (y>>>1)) ^ String((x+y))) | k)); else s = (s + String((k?k:x))); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; arr[(new P((x|0)).v == i) & 3] = (y)['length']; o.c = y; s = (s + ((arr)['length'] ?? y)); s = (s + (String(((x%5) >> (x|0)))?((y)['b'] & (x%5)):new P((x|0)).v)); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + k); s = (s + (((x+y) << (y>>>1)) && String(((x|0)?(y>>>1):0.5)))); s = (s + (cap = (cap + k) | 0)); o.c = x; return s; }
function f3(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (y>>>1)); arr[((((x*y)?x:(x+y)) | i) >> String((k && 3))) & 3] = 3; s = (s + (cap = (cap + (arr)['b']) | 0)); s = (s + (((y ^ 7)?(7 == (x%5)):3) - (o)['1'])); return s; }
const A = [4294967296,null,-3,0.25,new Q(1)]; const B = [undefined,false,0,[],{b:2,a:1}]; const C = [false,4294967296,2147483647,'a',{b:2,a:1}]; const A2 = ['bc',{a:1,b:2},0.25,'a',1.5]; const B2 = [0.25,[1.5,2,'x'],-2147483648,7,[]]; const C2 = [new P(2.5),{a:1,b:2,c:3},'',new P(2.5),NaN];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f3(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
