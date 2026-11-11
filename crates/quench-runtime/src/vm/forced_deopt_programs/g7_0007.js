function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; for (let j = 0; j < 6; j++) s = (s + (y)['1']); for (let j = 0; j < 6; j++) s = (s + ((x*y) < String((y>>>1)))); s = (s + (((x & (x*y))?0.5:((y>>>1) | y)) >>> (new P(y).v & ((y>>>1) && i)))); s = (s + (cap = (cap + i) | 0)); s = (s + (x && (arr)['a'])); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; try { s = (s + (x)['1']); } catch (e) { s = s + e.name; } s = (s + 3); if (((o)['length'] >>> ((y == i) & i))) s = (s + (((0.5?(y>>>1):(x|0)) * i) * (y>>>1))); else s = (s + String((x+y))); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (cap = (cap + (String((x - (x%5))) | ((arr)['1']?x:(k || k)))) | 0)); s = (s + (((x*y) ^ (0.5 * 0.5)) < (k >>> new P(k).v))); return s; }
function f3(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (cap = (cap + 7) | 0)); s = (s + (((x|0) || (x*y)) % (y <= Math.sqrt(x)))); o.b = ((y)['0'] * y); s = (s + ((((x%5)?(y>>>1):y) & (i?3:y))?String(y):(x+y))); for (let j = 0; j < 1; j++) s = (s + ((k < (o)['0']) - i)); return s; }
const A = [-3,[],4294967296,new P(3),NaN]; const B = [new P(3),{a:1,b:2},true,4294967296,new P(3)]; const C = ['',[],'bc',7,{b:2,a:1}]; const A2 = [2,0,0,{a:1,b:2,c:3},true]; const B2 = [-0,'a',new P(2.5),-2147483648,{a:1,b:2,c:3}]; const C2 = [7,1,{a:'s',b:null},'a',2];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f3(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
