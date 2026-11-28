function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + ((Math.round((x*y)) + (o)['b']) | (3 && (arr)['0']))); try { s = (s + (Math.round((y << x)) << new P((3 | i)).v)); } catch (e) { s = s + e.name; } o.a = (x|0); if (Math.trunc((((x+y) ^ x) * ((x+y) * y)))) s = (s + y); else s = (s + ((((x%5)?k:k) + (y <= (y>>>1))) || (x*y))); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; for (let j = 0; j < 1; j++) s = (s + (i >> (new P((x+y)).v < ((y>>>1)?(x%5):(x*y))))); s = (s + ((x*y) == (((y>>>1) && 3)?3:Math.abs(0.5)))); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; try { s = (s + (((0.5 < i) === (k >> (x%5)))?(String((x+y)) << (x*y)):(new P(0.5).v >> new P((x*y)).v))); } catch (e) { s = s + e.name; } o.b = new P(((x|0) <= (7 < (x+y)))).v; o.b = String(((x+y) != (k >> 3))); arr[((o)['1'] + ((x*y) << (x*y))) & 3] = ((x|0) | ((0.5 ?? 0.5)?(y>>>1):String(7))); return s; }
const A = [[1.5,2,'x'],-3,[1.5,2,'x'],NaN,1.5]; const B = [7,new P(3),new P(2.5),2,7]; const C = ['a',0,true,4294967296,0.25]; const A2 = [null,-2147483648,2,[1.5,2,'x'],'a']; const B2 = [4294967296,0,{a:1,b:2},1.5,'a']; const C2 = [-2147483648,-2147483648,1,2147483647,NaN];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
