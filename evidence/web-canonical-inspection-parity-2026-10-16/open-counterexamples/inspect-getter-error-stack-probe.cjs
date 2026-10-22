
const inspect=require('node:util').inspect,out=[];
class X { constructor(){this._y=123}get y(){return this._y} }
out.push(inspect(new X(),{getters:true,showHidden:true}));
class A { constructor(B){this.B=B}get b(){return this.B} }
class B { constructor(){this.A=new A(this)}get a(){return this.A} }
out.push(inspect(new B(),{depth:1,getters:true,showHidden:true}));
const value={get x(){throw Error('getter failed')},get y(){return {nested:true}},set z(value){}};
for(const getters of [false,true,'get','set'])out.push(inspect(value,{getters,sorted:true,compact:false}));
const d=Object.getOwnPropertyDescriptor(inspect,'defaultOptions');out.push([d.enumerable,d.configurable,d.get.name,d.set.name]);
for(const options of [null,1,'x',Symbol('x'),()=>{}])try{inspect.defaultOptions=options;out.push('ok')}catch(e){out.push([e.name,e.code,e.message])}
console.log(JSON.stringify(out));
