function capture(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name,error.message]}}
class Mixed {
 #instance=1;static #static=2;
 read(){const object=this;return [capture(()=>eval('this.#instance')),capture(()=>eval('Mixed.#static')),capture(function(){return eval('object.#instance')})]}
 static read(object){return [capture(()=>eval('#instance in object')),capture(()=>eval('Mixed.#static'))]}
}
const object=new Mixed();console.log(JSON.stringify([object.read(),Mixed.read(object)]));
