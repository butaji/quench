'use strict';
// console.log of plain objects and short arrays of primitives, as `inspect` writes them with
// default options on one line or one entry per line, and of every value that path leaves to
// `inspect`: nesting, holes, accessors, quoted keys, escaped strings, other prototypes and
// `util.inspect.custom`.
console.log({a:1});
console.log({});
console.log([]);
console.log([1,2,3]);
console.log([1,'two',null,undefined,true,-0]);
console.log([1,2,3,4,5,6,7]);
console.log({a:1,b:'x',c:null,d:undefined,e:false,f:10n,g:-0,h:NaN});
console.log({alpha:'aaaaaaaaaaaaaaaaaaaa',beta:'bbbbbbbbbbbbbbbbbbbbbbb',gamma:'cccccccccccccccccc'});
console.log({long:'x'.repeat(80)});
console.log({s:"it's"}, {t:'a"b'}, {u:'tab\there'}, {v:'é'});
console.log({'a-b':1}, {1:2}, {__proto__x:1});
console.log('pre', {a:1}, [2], 3);
console.log({a:{b:1}});
console.log([[1]]);
console.log([1,,3]);
var arr=[1,2]; arr.x=3; console.log(arr);
console.log(Object.create(null));
var A=class{constructor(){this.a=1}}; console.log(new A());
var g={}; Object.defineProperty(g,'x',{get(){return 1},enumerable:true}); console.log(g);
var h={}; Object.defineProperty(h,'hidden',{value:1,enumerable:false}); h.shown=2; console.log(h);
console.log(Object.setPrototypeOf(new Date(0), Object.prototype));
console.log({constructor:1});
console.log({f(){}});
console.log((function(){return arguments})(1,2));
console.log({a:1,b:2,c:3,d:4,e:5,f:6,g:7,h:8,i:9,j:10,k:11,l:12,m:13,n:14,o:15,p:16,q:17});
console.log({k0:0,k1:1,k2:2,k3:3,k4:4,k5:5,k6:6,k7:7,k8:8,k9:9,k10:10,k11:11,k12:12,k13:13});
console.log({e:1}, [3]);
var c={a:1}; c[Symbol.for('nodejs.util.inspect.custom')]=()=>'CUSTOM'; console.log(c);
Object.prototype[Symbol.for('nodejs.util.inspect.custom')]=function(){return 'PROTO'}; console.log({z:1}); delete Object.prototype[Symbol.for('nodejs.util.inspect.custom')];
console.log({x:1.5e300,y:-1e-7,z:Infinity});
