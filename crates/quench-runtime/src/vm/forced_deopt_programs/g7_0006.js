function P(v) { this.v = v; } function Q(v) { this.v = v; this.w = v; }
let cap = 0;
let h = 0;
function mix(v) { let t = String(v); for (let c = 0; c < t.length; c++) h = (h * 31 + t.charCodeAt(c)) | 0; }
function f0(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; s = (s + (String((arr)['0'])?0.5:y)); o.b = x; arr[Math.round((x)['b']) & 3] = (Math.abs(7) * (o)['b']); for (let j = 0; j < 1; j++) s = (s + (x%5)); return s; }
function f1(x, y, i) { let s = 0; let k = i & 7; let o = (typeof x === 'object' && x) ? x : {a:1,b:2}; let arr = [1,2,3,4]; if (7) s = (s + i); else s = (s + new P(Math.trunc(y)).v); s = (s + ((x)['1'] << ((3 * k)?(y>>>1):0.5))); s = (s + ((x+y) != (x+y))); o.a = (Math.abs((0.5 & (x%5))) ^ ((x|0) != (o)['c'])); return s; }
const A = ['bc',[1,2,3],{b:2,a:1},'bc',4294967296]; const B = [1.5,4294967296,'bc',{a:1,b:2,c:3},2]; const C = [{a:1,b:2,c:3},'','',new Q(1),new Q(1)]; const A2 = [false,'a','a',-2147483648,'bc']; const B2 = [false,[1,2,3],0.25,{a:1,b:2},{a:'s',b:null}]; const C2 = [-3,false,7,new Q(1),7];
for (let i = 0; i < 300; i++) {
    const late = i >= 150;
    const pa = late ? A2 : A, pb = late ? B2 : B;
    try { mix(f0(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
    try { mix(f1(pa[i % 5], pb[(i >> 1) % 5], i)); } catch (e) { mix(e.name); }
}
globalThis.answer = h * 1000003 + cap;
