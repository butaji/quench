function outcome(fn){try{return ['value',fn()]}catch(e){return ['throw',e.name]}}
const out=[];
class Instance {#field=1;#method(){}get #accessor(){return 2}static has(object){return [#field in object,#method in object,#accessor in object]}}
out.push(['prototype',Instance.has(Instance.prototype)],['instance',Instance.has(new Instance())]);
class Static {static before=[#field in this,#method in this,#accessor in this];static #field=1;static #method(){}static get #accessor(){return 2}static after=[#field in this,#method in this,#accessor in this]}
out.push(['static',Static.before,Static.after]);
console.log(JSON.stringify(out));
console.log(JSON.stringify(outcome(()=>{class Pair {#data=1;get #value(){return this.#data}set #value(value){this.#data=value}read(){this.#value=2;return this.#value}}return new Pair().read()})));
