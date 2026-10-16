function outcome(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name]}}
const out=[];
class Instance {#field=1;#method(){return 2}get #accessor(){return 3}static has(object){return [#field in object,#method in object,#accessor in object]}}
out.push(['prototype',Instance.has(Instance.prototype)],['instance',Instance.has(new Instance())]);
class Static {static before=[#field in this,#method in this,#accessor in this];static #field=1;static #method(){}static get #accessor(){return 2}static after=[#field in this,#method in this,#accessor in this]}
out.push(['static',Static.before,Static.after]);
class Earlier {static #first=[#first in this,#second in this];static #second=[#first in this,#second in this];static read(){return [this.#first,this.#second]}}
out.push(['order',Earlier.read()]);
class Pair {#data=1;get #value(){return this.#data}set #value(value){this.#data=value}read(){this.#value=2;return this.#value}static has(object){return #value in object}}
out.push(['pair',outcome(()=>new Pair().read()),Pair.has(Pair.prototype)]);
class ReturnValue {constructor(object){return object}}
class Stamp extends ReturnValue {#x=7;static has(object){return #x in object}static read(object){return object.#x}}
const prototype=Stamp.prototype;out.push(['stamp-before',Stamp.has(prototype)]);
new Stamp(prototype);out.push(['stamp-after',Stamp.has(prototype),Stamp.read(prototype)]);
out.push(['stamp-twice',outcome(()=>new Stamp(prototype)),Stamp.read(prototype)]);
console.log(JSON.stringify(out));
