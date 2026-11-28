function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + ((x%5)?(((x%5) ^ y) + (y>>>1)):(k?((y>>>1) | y):Math.floor(0.5)))); s = (s + String(((0.5 | 0.5) ^ ((x+y)?y:(x|0))))); o.b = k; try { s = (s + ((Math.abs(y) >> String((x%5)))?String(0.5):(x|0))); } catch (e) { s = s + e.name; } return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; for (let j = 0; j < 1; j++) s = (s + Math.floor((y | (k >> k)))); s = (s + (String((k != (y>>>1))) + String((0.5 ?? (x%5))))); arr[y & 3] = (x%5); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; o.c = (y)['length']; s = (s + (cap = (cap + x) | 0)); s = (s + Math.floor((x+y))); return s; }
const A = [{a:1,b:2,c:3},-2147483648,'',[1,2,3],{b:2,a:1}]; const B = [1,4294967296,{b:2,a:1},0.25,{a:'s',b:null}]; const C = [2,[],{b:2,a:1},'a',-2147483648]; const A2 = [new P(3),1,[1,2,3],4294967296,1.5]; const B2 = [new P(2.5),'a',new P(3),new Q(1),true]; const C2 = [undefined,new Q(1),0,null,{a:1,b:2}];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
