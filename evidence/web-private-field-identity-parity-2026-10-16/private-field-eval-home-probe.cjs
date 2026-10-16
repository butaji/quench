class Base {constructor(object){return object}}
function make(){return class extends Base {#x=1;read(){return this.#x}evaluate(){return eval('this.#x')}}}
const A=make(),object={};new A(object);const out=[];
function capture(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name,error.message]}}
out.push(capture(()=>A.prototype.evaluate.call(object)));try{new A(object)}catch(error){}
out.push(capture(()=>A.prototype.evaluate.call(object)));out.push(A.prototype.read.call(object));
console.log(JSON.stringify(out));
