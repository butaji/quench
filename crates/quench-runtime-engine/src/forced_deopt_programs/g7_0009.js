function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; o.b = (((x & 7) >> (arr)['0'])?((i?(y>>>1):(x*y)) < 3):((y?(x%5):i) < ((y>>>1)?(x*y):0.5))); for (let j = 0; j < 3; j++) s = (s + String((Math.round(0.5) & new P(0.5).v))); s = (s + ((x)['c'] && (o)['a'])); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (x%5)); try { s = (s + (y)['c']); } catch (e) { s = s + e.name; } for (let j = 0; j < 6; j++) s = (s + new P(Math.floor((x|0))).v); if ((arr)['1']) s = (s + i); else s = (s + ((String((x+y)) % ((x+y) && i))?((x)['1'] && 3):String((0.5 * i)))); if ((arr)['0']) s = (s + (x)['a']); else s = (s + 7); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; for (let j = 0; j < 4; j++) s = (s + ((Math.floor(0.5) - ((x+y)?x:x)) < String(((y>>>1) || 3)))); s = (s + Math.floor(new P(0.5).v)); for (let j = 0; j < 1; j++) s = (s + (y?((x%5) || new P((x*y)).v):7)); s = (s + (cap = (cap + String(((o)['0'] == y))) | 0)); for (let j = 0; j < 2; j++) s = (s + (String(((x|0) >> x)) != (3 >>> (0.5 % 7)))); return s; }
const A = [4294967296,null,0.25,{a:1,b:2},7]; const B = [-2147483648,{b:2,a:1},'',new P(2.5),new P(2.5)]; const C = [-2147483648,'',4294967296,NaN,-3]; const A2 = [0,2147483647,'',{a:'s',b:null},'a']; const B2 = [2,0.25,1.5,2,-0]; const C2 = [false,[],undefined,-2147483648,NaN];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
