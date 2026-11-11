function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; try { s = (s + (y>>>1)); } catch (e) { s = s + e.name; } for (let j = 0; j < 6; j++) s = (s + k); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; try { s = (s + ((String(i)?new P((x|0)).v:x) === (x)['a'])); } catch (e) { s = s + e.name; } arr[(y>>>1) & 3] = (String(0.5) == ((0.5?x:(x*y)) ^ ((x%5) || y))); try { s = (s + (o)['b']); } catch (e) { s = s + e.name; } s = (s + (cap = (cap + y) | 0)); s = (s + (7?Math.floor((k & (x+y))):(x | ((x+y) >>> (y>>>1))))); return s; }
const A = [{b:2,a:1},2147483647,{a:1,b:2,c:3},0,'bc']; const B = [1,undefined,7,-2147483648,[1.5,2,'x']]; const C = [undefined,NaN,-3,true,new P(3)]; const A2 = [{a:1,b:2},{a:1,b:2},'a',new Q(1),{a:'s',b:null}]; const B2 = [new Q(1),new P(2.5),{b:2,a:1},-2147483648,[1.5,2,'x']]; const C2 = [7,[1,2,3],2147483647,'bc',-0];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
