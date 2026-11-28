function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (((k < 3) & (x%5))?(x*y):String((x|0)))); for (let j = 0; j < 4; j++) s = (s + x); s = (s + (cap = (cap + (y)['b']) | 0)); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (arr)['length']); s = (s + ((((y>>>1) - k)?(7 == x):k) == x)); s = (s + String(((y)['1']?(o)['0']:((x%5) === 3)))); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (x+y)); for (let j = 0; j < 4; j++) s = (s + String(((x|0) >>> String(x)))); if ((arr)['b']) s = (s + (x|0)); else s = (s + String(((x)['b'] * (x+y)))); for (let j = 0; j < 6; j++) s = (s + (x*y)); s = (s + new P((x)['1']).v); return s; }
function f3(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; arr[String(((0.5?k:3) * ((x%5)?3:(x%5)))) & 3] = y; s = (s + (((x%5) & ((x*y) + 3)) >> k)); try { s = (s + (((y % 0.5) * k) || 7)); } catch (e) { s = s + e.name; } arr[((i - (y >>> k)) ^ ((y)['length'] - new P(x).v)) & 3] = (String(((x*y) * x))?((arr)['0'] === 0.5):((arr)['a'] >> 3)); s = (s + (x|0)); return s; }
const A = [[1,2,3],'a',null,{a:'s',b:null},2147483647]; const B = [1.5,0,null,2147483647,undefined]; const C = [false,'',false,4294967296,{a:1,b:2}]; const A2 = ['a','a',false,4294967296,null]; const B2 = [{a:'s',b:null},'bc',7,new Q(1),0.25]; const C2 = [true,[],2147483647,'bc',new Q(1)];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f3(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
