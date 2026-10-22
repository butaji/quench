function outcome(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name]}}
const out=[];
class Mixed {
 #instance=1; static #static=2;
 read(){const object=this;return [outcome(()=>eval('this.#instance')),outcome(()=>eval('Mixed.#static')),outcome(function(){return eval('object.#instance')})]}
 static read(object){return [outcome(()=>eval('#instance in object')),outcome(()=>eval('Mixed.#static'))]}
 closure(){const object=this;return function(){return eval('object.#instance')}}
 negative(){return function(){return ['super.x','super()','return 1','#missing in {}','}); } extra() { 7 } __eval() { (function() { 9'].map(source=>outcome(()=>eval(source)))}}
 wrapper(){return outcome(()=>eval('} extra() { 7 } __eval() { 9'))}
 target(){return function(){return eval('new.target === undefined')}}
}
const object=new Mixed();out.push(['mixed',object.read(),Mixed.read(object)]);
out.push(['escaped',object.closure()()]);out.push(['negative',object.negative()()]);out.push(['target',object.target()()]);out.push(['wrapper',object.wrapper()]);
class Outer {
 #value=3;
 create(){const outer=this;return class Inner {
  #value=4;
  read(){const inner=this;return function(){return [eval('inner.#value'),outcome(()=>eval('outer.#value'))]}}
 }}
}
const Inner=new Outer().create();out.push(['shadow',new Inner().read()()]);
class OtherOuter {
 #outer=5;
 create(){const outer=this;return class {
  #inner=6;
  read(){const inner=this;return function(){return [eval('inner.#inner'),eval('outer.#outer')]}}
 }}
}
const OtherInner=new OtherOuter().create();out.push(['outer',new OtherInner().read()()]);
console.log(JSON.stringify(out));
