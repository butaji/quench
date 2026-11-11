function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (x+y)); try { s = (s + (new P((0.5?k:(x|0))).v ^ y)); } catch (e) { s = s + e.name; } o.c = new P((x?(3 ?? (y>>>1)):0.5)).v; s = (s + ((new P((x%5)).v >> new P(k).v) || ((y>>>1) | (y?0.5:(x|0))))); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; try { s = (s + (o)['a']); } catch (e) { s = s + e.name; } s = (s + Math.floor((y>>>1))); for (let j = 0; j < 3; j++) s = (s + String(((3?(x*y):(x%5))?k:new P((x%5)).v))); return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; arr[((0.5?(x*y):((x+y) * (x+y))) ?? Math.abs(k)) & 3] = k; s = (s + 0.5); s = (s + y); return s; }
const A = [-0,2,2147483647,new P(3),1]; const B = [7,2147483647,null,1.5,4294967296]; const C = [{a:1,b:2},{a:1,b:2,c:3},true,1.5,false]; const A2 = [0.25,{b:2,a:1},[1,2,3],false,-2147483648]; const B2 = [[1,2,3],4294967296,[],0.25,undefined]; const C2 = [new Q(1),4294967296,true,'a',0.25];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
