function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (String((7 ?? (x*y))) ?? String((arr)['a']))); s = (s + (x|0)); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; for (let j = 0; j < 1; j++) s = (s + ((String(3) < (3?(x+y):7)) << ((i >> i) || (i?(x%5):(x|0))))); try { s = (s + (y>>>1)); } catch (e) { s = s + e.name; } return s; }
function f2(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; arr[((0.5 == 3) << x) & 3] = k; s = (s + String((x)['a'])); return s; }
function f3(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (y?y:(x%5))); o.a = (x+y); for (let j = 0; j < 6; j++) s = (s + (arr)['0']); return s; }
const A = [false,4294967296,'bc',new P(2.5),0.25]; const B = [false,false,{a:1,b:2},false,-3]; const C = [{a:1,b:2},{b:2,a:1},-3,[1.5,2,'x'],-2147483648]; const A2 = [{a:1,b:2},'a',-2147483648,'',1.5]; const B2 = [new P(2.5),{b:2,a:1},'bc','',-3]; const C2 = [1.5,false,'',[],'a'];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f2(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f3(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
