function outcome(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name]}}
const out=[];
class Base {static value=7;static get receiver(){return this.name}static set setValue(value){this.received=value}}
class Derived extends Base {
 static #value=3;
 static {
  out.push(['direct',outcome(()=>super.value),outcome(()=>eval('super.value')),outcome(()=>eval('super.receiver')),outcome(()=>eval('this.#value'))]);
  out.push(['arrow',outcome(()=>eval('super.value'))]);
  out.push(['ordinary',outcome(function(){return eval('super.value')})]);
  out.push(['arguments',outcome(()=>eval('typeof arguments')),outcome(()=>eval('(()=>typeof arguments)()')),outcome(()=>eval('(function(){return typeof arguments})()'))]);
  out.push(['target',outcome(()=>eval('new.target === undefined'))]);
  out.push(['setter',outcome(()=>eval('super.setValue=9')),this.received]);
  out.push(['strict',outcome(()=>eval('quenchStaticBlockUnbound=1')),outcome(()=>{Object.defineProperty(this,'locked',{value:1});this.locked=2})]);
  var local=11;out.push(['local',eval('local')]);
 }
 static {out.push(['separate',typeof local])}
}
console.log(JSON.stringify(out));
