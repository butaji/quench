function outcome(fn){try{return ['value',fn()]}catch(e){return ['throw',e.name]}}
const out=[];
class C {static #x=3;static {out.push(outcome(()=>eval('C.#x')),outcome(()=>eval('new.target')),outcome(()=>eval('super.name')),outcome(()=>eval('return 1')))}}
console.log(JSON.stringify(out));
